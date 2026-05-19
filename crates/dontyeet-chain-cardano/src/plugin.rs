//! Cardano chain plugin — bundles all Cardano capabilities into a
//! single [`ChainPlugin`] implementation.

use std::any::Any;

use async_trait::async_trait;

use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::secret::Seed;
use dontyeet_primitives::traits::{ChainPlugin, FeeEstimator, KeyDeriver, TransactionBroadcaster};
use dontyeet_primitives::transaction::{
    RpcHealthResult, StandardizedFee, StandardizedFeeTier, TxHash,
};
use dontyeet_primitives::{Address, Amount, DontYeetWalletError, Result};

use dontyeet_chain::FnSigner;
use dontyeet_network::{HttpClient, NetworkCatalog, ReqwestClient};

use crate::balance::CardanoBalanceFetcher;
use crate::broadcast::CardanoBroadcaster;
use crate::config::{CardanoConfig, default_cardano_config};
use crate::fees::CardanoFeeEstimator;
use crate::history::CardanoHistoryFetcher;
use crate::keys::{CardanoAddressEncoder, CardanoKeyDeriver};

/// A complete Cardano chain integration.
///
/// Handles mainnet, preprod, and preview. Uses Ed25519 keypairs,
/// enterprise addresses (blake2b-224), and Blockfrost REST API.
pub struct CardanoChainPlugin {
    config: CardanoConfig,
    key_deriver: CardanoKeyDeriver,
    address_encoder: CardanoAddressEncoder,
    fee_estimator: CardanoFeeEstimator,
    balance_fetcher: CardanoBalanceFetcher,
    tx_signer: FnSigner,
    broadcaster: CardanoBroadcaster,
    network_catalog: NetworkCatalog,
    history_fetcher: CardanoHistoryFetcher,
}

impl CardanoChainPlugin {
    /// Create a new Cardano chain plugin from the given configuration.
    #[must_use]
    pub fn new(config: CardanoConfig) -> Self {
        let key_deriver = CardanoKeyDeriver::new(&config);
        let fee_estimator =
            CardanoFeeEstimator::new(&config.api_urls, config.blockfrost_project_id.clone());
        let balance_fetcher =
            CardanoBalanceFetcher::new(&config.api_urls, config.blockfrost_project_id.clone());
        let tx_signer = FnSigner::new(|msg, key| {
            crate::signing::sign_tx_body(msg, key).map(|sig| sig.to_vec())
        });
        let broadcaster =
            CardanoBroadcaster::new(&config.api_urls, config.blockfrost_project_id.clone());
        let network_catalog = NetworkCatalog::new(
            config.networks.clone(),
            config.explorer_urls.clone(),
            config.api_urls.clone(),
        );
        let history_fetcher =
            CardanoHistoryFetcher::new(&config.api_urls, config.blockfrost_project_id.clone());

        Self {
            config,
            key_deriver,
            address_encoder: CardanoAddressEncoder,
            fee_estimator,
            balance_fetcher,
            tx_signer,
            broadcaster,
            network_catalog,
            history_fetcher,
        }
    }
}

#[async_trait]
impl ChainPlugin for CardanoChainPlugin {
    dontyeet_chain::chain_plugin_accessors! {
        config_field    = config,
        key_deriver     = key_deriver,
        address_encoder = address_encoder,
        balance_fetcher = balance_fetcher,
        fee_estimator   = fee_estimator,
        signer          = tx_signer,
        broadcaster     = broadcaster,
        network         = network_catalog,
    }

    fn tx_builder_any(&self) -> &dyn Any {
        self
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "elapsed health-check time fits in u64 ms; network timeouts cap it well below u128 saturation"
    )]
    async fn check_rpc_health(&self, network: &NetworkId) -> Result<RpcHealthResult> {
        #[derive(serde::Deserialize)]
        struct BlockResponse {
            height: Option<u64>,
        }

        let Some(urls) = self.config.api_urls.get(network) else {
            return Ok(RpcHealthResult {
                reachable: false,
                response_time_ms: 0,
                latest_block: None,
                error: Some(format!("no API URLs for {network}")),
            });
        };

        let Some(base) = urls.first() else {
            return Ok(RpcHealthResult {
                reachable: false,
                response_time_ms: 0,
                latest_block: None,
                error: Some("API URL list is empty".into()),
            });
        };

        let Ok(blocks_url) = base.join("/blocks/latest") else {
            return Ok(RpcHealthResult {
                reachable: false,
                response_time_ms: 0,
                latest_block: None,
                error: Some("URL join error".into()),
            });
        };

        let start = std::time::Instant::now();
        let result = async {
            let client =
                ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
            let headers =
                crate::auth::project_id_headers(self.config.blockfrost_project_id.as_deref());
            let response = client
                .get_with_headers(&blocks_url, &headers)
                .await
                .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
            let block: BlockResponse = response
                .json()
                .map_err(|e| DontYeetWalletError::Network(format!("block parse error: {e}")))?;
            Ok::<Option<u64>, DontYeetWalletError>(block.height)
        }
        .await;
        let elapsed = start.elapsed().as_millis() as u64;

        match result {
            Ok(height) => Ok(RpcHealthResult {
                reachable: true,
                response_time_ms: elapsed,
                latest_block: height,
                error: None,
            }),
            Err(e) => Ok(RpcHealthResult {
                reachable: false,
                response_time_ms: elapsed,
                latest_block: None,
                error: Some(e.to_string()),
            }),
        }
    }

    async fn estimate_fee_display(&self, network: &NetworkId) -> Result<StandardizedFeeTier> {
        /// Format lovelace as a human-readable ADA string.
        fn ada_label(lovelace: u64) -> String {
            let whole = lovelace / 1_000_000;
            let frac = lovelace % 1_000_000;
            format!("~{whole}.{frac:06} ADA")
        }

        let tiers = self.fee_estimator.estimate_fees(network).await?;

        Ok(StandardizedFeeTier {
            slow: StandardizedFee {
                label: ada_label(tiers.slow.lovelace),
                native_amount: tiers.slow.lovelace.to_string(),
            },
            standard: StandardizedFee {
                label: ada_label(tiers.standard.lovelace),
                native_amount: tiers.standard.lovelace.to_string(),
            },
            fast: StandardizedFee {
                label: ada_label(tiers.fast.lovelace),
                native_amount: tiers.fast.lovelace.to_string(),
            },
        })
    }

    async fn send_transfer(
        &self,
        seed: &Seed,
        to: &Address,
        amount: &Amount,
        network: &NetworkId,
    ) -> Result<TxHash> {
        let keypair = self.key_deriver.derive_keypair(seed, network)?;
        let private_key = keypair
            .private_key()
            .ok_or_else(|| DontYeetWalletError::Chain("no private key derived".into()))?;

        let signed_tx = crate::transfer::build_signed_transfer(
            &self.config.api_urls,
            self.config.blockfrost_project_id.as_deref(),
            &keypair.address,
            to,
            amount,
            private_key,
            network,
        )
        .await?;

        self.broadcaster.broadcast(&signed_tx, network).await
    }

    async fn fetch_history(
        &self,
        address: &Address,
        network: &NetworkId,
        limit: usize,
    ) -> Result<Vec<dontyeet_primitives::transaction::TxHistoryItem>> {
        self.history_fetcher
            .fetch(address.as_str(), network, limit)
            .await
    }
}

/// Create a [`CardanoChainPlugin`] with the default configuration.
///
/// Includes mainnet, preprod, and preview networks with Blockfrost
/// REST API v0 endpoints.
#[must_use]
pub fn cardano_plugin() -> CardanoChainPlugin {
    CardanoChainPlugin::new(default_cardano_config())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dontyeet_primitives::chain::ChainId;

    #[test]
    fn factory_produces_cardano_chain_id() {
        let plugin = cardano_plugin();
        assert_eq!(*plugin.chain_id(), ChainId::Cardano);
    }

    #[test]
    fn factory_has_three_networks() {
        let plugin = cardano_plugin();
        assert_eq!(plugin.network_provider().networks().len(), 3);
    }

    #[test]
    fn native_asset_is_ada() {
        let plugin = cardano_plugin();
        assert_eq!(plugin.native_asset().symbol, "ADA");
        assert_eq!(plugin.native_asset().decimals, 6);
    }

    #[test]
    fn default_network_is_mainnet() {
        let plugin = cardano_plugin();
        let default = plugin.network_provider().default_network();
        assert_eq!(default.id.as_ref(), "cardano-mainnet");
    }
}

// Rust guideline compliant 2026-05-02
