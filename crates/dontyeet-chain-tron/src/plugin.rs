//! TRON chain plugin -- bundles all TRON capabilities into a single
//! [`ChainPlugin`] implementation.

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

use crate::balance::TronBalanceFetcher;
use crate::broadcast::TronBroadcaster;
use crate::config::{TronConfig, default_tron_config};
use crate::fees::TronFeeEstimator;
use crate::history::TronHistoryFetcher;
use crate::keys::{TronAddressEncoder, TronKeyDeriver};

/// A complete TRON chain integration.
///
/// Handles mainnet, Shasta testnet, and Nile testnet. Uses `Base58Check`
/// addresses with the `0x41` version byte and the `TronGrid` REST API
/// for all network operations.
pub struct TronChainPlugin {
    config: TronConfig,
    key_deriver: TronKeyDeriver,
    address_encoder: TronAddressEncoder,
    fee_estimator: TronFeeEstimator,
    balance_fetcher: TronBalanceFetcher,
    tx_signer: FnSigner,
    broadcaster: TronBroadcaster,
    network_catalog: NetworkCatalog,
    history_fetcher: TronHistoryFetcher,
}

impl TronChainPlugin {
    /// Create a new TRON chain plugin from the given configuration.
    #[must_use]
    pub fn new(config: TronConfig) -> Self {
        let key_deriver = TronKeyDeriver::new(&config);
        let fee_estimator = TronFeeEstimator::new(&config.api_urls);
        let balance_fetcher = TronBalanceFetcher::new(&config.api_urls);
        let tx_signer = FnSigner::new(crate::signing::sign_raw_data);
        let broadcaster = TronBroadcaster::new(&config.api_urls);
        let network_catalog = NetworkCatalog::new(
            config.networks.clone(),
            config.explorer_urls.clone(),
            config.api_urls.clone(),
        );
        let history_fetcher = TronHistoryFetcher::new(&config.api_urls);

        Self {
            config,
            key_deriver,
            address_encoder: TronAddressEncoder,
            fee_estimator,
            balance_fetcher,
            tx_signer,
            broadcaster,
            network_catalog,
            history_fetcher,
        }
    }

    /// Borrow the underlying configuration.
    #[must_use]
    pub fn config(&self) -> &TronConfig {
        &self.config
    }
}

#[async_trait]
impl ChainPlugin for TronChainPlugin {
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
        // TRON transaction building is deferred to a future iteration.
        // Placeholder -- returns self as Any for future downcasting.
        self
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "elapsed health-check time fits in u64 ms; network timeouts cap it well below u128 saturation"
    )]
    async fn check_rpc_health(&self, network: &NetworkId) -> Result<RpcHealthResult> {
        #[derive(serde::Deserialize)]
        struct RawData {
            number: Option<u64>,
        }
        #[derive(serde::Deserialize)]
        struct BlockHeader {
            raw_data: RawData,
        }
        #[derive(serde::Deserialize)]
        struct NowBlockResponse {
            block_header: Option<BlockHeader>,
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

        let Ok(block_url) = base.join("/wallet/getnowblock") else {
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
                .post_json(&block_url, &serde_json::json!({}))
                .await
                .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
            let block: NowBlockResponse = response
                .json()
                .map_err(|e| DontYeetWalletError::Network(format!("block parse error: {e}")))?;
            let number = block.block_header.and_then(|h| h.raw_data.number);
            Ok::<Option<u64>, DontYeetWalletError>(number)
        }
        .await;
        let elapsed = start.elapsed().as_millis() as u64;

        match result {
            Ok(number) => Ok(RpcHealthResult {
                reachable: true,
                response_time_ms: elapsed,
                latest_block: number,
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
        /// Convert TRON bandwidth points to SUN and format as TRX.
        ///
        /// 1 bandwidth point costs 1000 SUN; 1 TRX = 1,000,000 SUN.
        fn trx_label(bandwidth_points: u64) -> String {
            let sun = bandwidth_points.saturating_mul(1000);
            let whole = sun / 1_000_000;
            let frac = sun % 1_000_000;
            format!("~{whole}.{frac:06} TRX")
        }

        /// Bandwidth points to SUN string.
        fn sun_amount(bandwidth_points: u64) -> String {
            bandwidth_points.saturating_mul(1000).to_string()
        }

        let tiers = self.fee_estimator.estimate_fees(network).await?;

        Ok(StandardizedFeeTier {
            slow: StandardizedFee {
                label: trx_label(tiers.slow.bandwidth_points),
                native_amount: sun_amount(tiers.slow.bandwidth_points),
            },
            standard: StandardizedFee {
                label: trx_label(tiers.standard.bandwidth_points),
                native_amount: sun_amount(tiers.standard.bandwidth_points),
            },
            fast: StandardizedFee {
                label: trx_label(tiers.fast.bandwidth_points),
                native_amount: sun_amount(tiers.fast.bandwidth_points),
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

        // 2. Build and sign transfer via TronGrid API
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

/// Create a [`TronChainPlugin`] with the default configuration.
///
/// Includes mainnet, Shasta testnet, and Nile testnet networks with
/// `TronGrid` REST API endpoints.
#[must_use]
pub fn tron_plugin() -> TronChainPlugin {
    TronChainPlugin::new(default_tron_config())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dontyeet_primitives::chain::ChainId;

    #[test]
    fn factory_produces_tron_chain_id() {
        let plugin = tron_plugin();
        assert_eq!(*plugin.chain_id(), ChainId::Tron);
    }

    #[test]
    fn factory_has_three_networks() {
        let plugin = tron_plugin();
        assert_eq!(plugin.network_provider().networks().len(), 3);
    }

    #[test]
    fn native_asset_is_trx() {
        let plugin = tron_plugin();
        assert_eq!(plugin.native_asset().symbol, "TRX");
        assert_eq!(plugin.native_asset().decimals, 6);
    }

    #[test]
    fn default_network_is_mainnet() {
        let plugin = tron_plugin();
        let default = plugin.network_provider().default_network();
        assert_eq!(default.id.as_ref(), "tron-mainnet");
    }
}

// Rust guideline compliant 2026-05-02
