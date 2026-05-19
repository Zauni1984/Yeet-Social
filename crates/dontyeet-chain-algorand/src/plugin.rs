//! Algorand chain plugin — bundles all Algorand capabilities into a
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

use crate::balance::AlgoBalanceFetcher;
use crate::broadcast::AlgoBroadcaster;
use crate::config::{AlgoConfig, default_algo_config};
use crate::fees::AlgoFeeEstimator;
use crate::history::AlgoHistoryFetcher;
use crate::keys::{AlgoAddressEncoder, AlgoKeyDeriver};

/// A complete Algorand chain integration.
///
/// Handles mainnet and testnet. Uses Ed25519 keypairs and the
/// Algod REST v2 API (Nodely) for all network operations.
pub struct AlgoChainPlugin {
    config: AlgoConfig,
    key_deriver: AlgoKeyDeriver,
    address_encoder: AlgoAddressEncoder,
    fee_estimator: AlgoFeeEstimator,
    balance_fetcher: AlgoBalanceFetcher,
    tx_signer: FnSigner,
    broadcaster: AlgoBroadcaster,
    network_catalog: NetworkCatalog,
    history_fetcher: AlgoHistoryFetcher,
}

impl AlgoChainPlugin {
    /// Create a new Algorand chain plugin from the given configuration.
    #[must_use]
    pub fn new(config: AlgoConfig) -> Self {
        let key_deriver = AlgoKeyDeriver::new(&config);
        let fee_estimator = AlgoFeeEstimator::new(&config.api_urls);
        let balance_fetcher = AlgoBalanceFetcher::new(&config.api_urls);
        let tx_signer = FnSigner::new(|msg, key| {
            crate::signing::sign_unsigned_payload(msg, key).map(|sig| sig.to_vec())
        });
        let broadcaster = AlgoBroadcaster::new(&config.api_urls);
        let network_catalog = NetworkCatalog::new(
            config.networks.clone(),
            config.explorer_urls.clone(),
            config.api_urls.clone(),
        );
        let history_fetcher = AlgoHistoryFetcher::new(&config.api_urls);

        Self {
            config,
            key_deriver,
            address_encoder: AlgoAddressEncoder,
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
impl ChainPlugin for AlgoChainPlugin {
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
        #[serde(rename_all = "kebab-case")]
        struct StatusResponse {
            last_round: u64,
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

        let Ok(status_url) = base.join("/v2/status") else {
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
            let response = client
                .get(&status_url)
                .await
                .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
            let status: StatusResponse = response
                .json()
                .map_err(|e| DontYeetWalletError::Network(format!("status parse error: {e}")))?;
            Ok::<u64, DontYeetWalletError>(status.last_round)
        }
        .await;
        let elapsed = start.elapsed().as_millis() as u64;

        match result {
            Ok(round) => Ok(RpcHealthResult {
                reachable: true,
                response_time_ms: elapsed,
                latest_block: Some(round),
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
        /// Format `microAlgos` as a human-readable ALGO string.
        fn algo_label(micro_algos: u64) -> String {
            let whole = micro_algos / 1_000_000;
            let frac = micro_algos % 1_000_000;
            format!("~{whole}.{frac:06} ALGO")
        }

        let tiers = self.fee_estimator.estimate_fees(network).await?;

        Ok(StandardizedFeeTier {
            slow: StandardizedFee {
                label: algo_label(tiers.slow.micro_algos),
                native_amount: tiers.slow.micro_algos.to_string(),
            },
            standard: StandardizedFee {
                label: algo_label(tiers.standard.micro_algos),
                native_amount: tiers.standard.micro_algos.to_string(),
            },
            fast: StandardizedFee {
                label: algo_label(tiers.fast.micro_algos),
                native_amount: tiers.fast.micro_algos.to_string(),
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
        // 1. Derive keys
        let keypair = self.key_deriver.derive_keypair(seed, network)?;
        let private_key = keypair
            .private_key()
            .ok_or_else(|| DontYeetWalletError::Chain("no private key derived".into()))?;

        // 2. Build and sign the payment transaction
        let signed_tx = crate::transfer::build_signed_transfer(
            &self.config.api_urls,
            &keypair.address,
            to,
            amount,
            private_key,
            network,
        )
        .await?;

        // 3. Broadcast
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

/// Create an [`AlgoChainPlugin`] with the default configuration.
///
/// Includes mainnet and testnet networks with Nodely Algod v2
/// REST API endpoints.
#[must_use]
pub fn algorand_plugin() -> AlgoChainPlugin {
    AlgoChainPlugin::new(default_algo_config())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dontyeet_primitives::chain::ChainId;

    #[test]
    fn factory_produces_algorand_chain_id() {
        let plugin = algorand_plugin();
        assert_eq!(*plugin.chain_id(), ChainId::Algorand);
    }

    #[test]
    fn factory_has_two_networks() {
        let plugin = algorand_plugin();
        assert_eq!(plugin.network_provider().networks().len(), 2);
    }

    #[test]
    fn native_asset_is_algo() {
        let plugin = algorand_plugin();
        assert_eq!(plugin.native_asset().symbol, "ALGO");
        assert_eq!(plugin.native_asset().decimals, 6);
    }

    #[test]
    fn default_network_is_mainnet() {
        let plugin = algorand_plugin();
        let default = plugin.network_provider().default_network();
        assert_eq!(default.id.as_ref(), "algorand-mainnet");
    }
}

// Rust guideline compliant 2026-05-02
