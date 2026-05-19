//! Bitcoin chain plugin — bundles all Bitcoin capabilities into a single
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

use crate::balance::BtcBalanceFetcher;
use crate::broadcast::BtcBroadcaster;
use crate::config::{BtcConfig, default_btc_config};
use crate::fees::BtcFeeEstimator;
use crate::history::BtcHistoryFetcher;
use crate::keys::{BtcAddressEncoder, BtcKeyDeriver};

/// A complete Bitcoin chain integration.
///
/// Handles mainnet, testnet4, and signet. Uses P2WPKH (segwit) addresses
/// by default and the Mempool.space REST API for all network operations.
pub struct BtcChainPlugin {
    config: BtcConfig,
    key_deriver: BtcKeyDeriver,
    address_encoder: BtcAddressEncoder,
    fee_estimator: BtcFeeEstimator,
    balance_fetcher: BtcBalanceFetcher,
    tx_signer: FnSigner,
    broadcaster: BtcBroadcaster,
    network_catalog: NetworkCatalog,
    history_fetcher: BtcHistoryFetcher,
}

impl BtcChainPlugin {
    /// Create a new Bitcoin chain plugin from the given configuration.
    #[must_use]
    pub fn new(config: BtcConfig) -> Self {
        let key_deriver = BtcKeyDeriver::new(&config);
        let address_encoder = BtcAddressEncoder::new(&config);
        let fee_estimator = BtcFeeEstimator::new(&config.api_urls);
        let balance_fetcher = BtcBalanceFetcher::new(&config.api_urls);
        let tx_signer = FnSigner::new(crate::signing::sign_p2wpkh_input);
        let broadcaster = BtcBroadcaster::new(&config.api_urls);
        let network_catalog = NetworkCatalog::new(
            config.networks.clone(),
            config.explorer_urls.clone(),
            config.api_urls.clone(),
        );
        let history_fetcher = BtcHistoryFetcher::new(&config.api_urls);

        Self {
            config,
            key_deriver,
            address_encoder,
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
    pub fn config(&self) -> &BtcConfig {
        &self.config
    }
}

#[async_trait]
impl ChainPlugin for BtcChainPlugin {
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
        // Bitcoin transaction building is complex (UTXO selection).
        // Placeholder — returns self as Any for future downcasting.
        self
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "elapsed health-check time fits in u64 ms; network timeouts cap it well below u128 saturation"
    )]
    async fn check_rpc_health(&self, network: &NetworkId) -> Result<RpcHealthResult> {
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

        let Ok(tip_url) = base.join("/blocks/tip/height") else {
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
                .get(&tip_url)
                .await
                .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
            let text = String::from_utf8(response.body)
                .map_err(|e| DontYeetWalletError::Network(format!("UTF-8 error: {e}")))?;
            let height: u64 = text
                .trim()
                .parse()
                .map_err(|e| DontYeetWalletError::Network(format!("height parse error: {e}")))?;
            Ok::<u64, DontYeetWalletError>(height)
        }
        .await;
        let elapsed = start.elapsed().as_millis() as u64;

        match result {
            Ok(block) => Ok(RpcHealthResult {
                reachable: true,
                response_time_ms: elapsed,
                latest_block: Some(block),
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

    /// Return Bitcoin fee estimates formatted for UI display.
    ///
    /// Queries `Mempool.space` for recommended fee rates and converts each
    /// tier into a [`StandardizedFee`] using a typical P2WPKH transaction
    /// size of ~140 vbytes.
    async fn estimate_fee_display(&self, network: &NetworkId) -> Result<StandardizedFeeTier> {
        /// Typical P2WPKH transaction size in virtual bytes (1-in / 2-out).
        const TYPICAL_VBYTES: u64 = 140;

        let tiers = self.fee_estimator.estimate_fees(network).await?;

        let to_display = |fees: &crate::fees::BtcFees| -> StandardizedFee {
            let rate = fees.sats_per_vbyte;
            let total_sats = rate.saturating_mul(TYPICAL_VBYTES);
            StandardizedFee {
                label: format!("~{rate} sat/vB ({total_sats} sats)"),
                native_amount: total_sats.to_string(),
            }
        };

        Ok(StandardizedFeeTier {
            slow: to_display(&tiers.slow),
            standard: to_display(&tiers.standard),
            fast: to_display(&tiers.fast),
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

/// Create a [`BtcChainPlugin`] with the default configuration.
///
/// Includes mainnet, testnet4, and signet networks with Mempool.space
/// REST API endpoints.
#[must_use]
pub fn bitcoin_plugin() -> BtcChainPlugin {
    BtcChainPlugin::new(default_btc_config())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dontyeet_primitives::chain::ChainId;

    #[test]
    fn factory_produces_bitcoin_chain_id() {
        let plugin = bitcoin_plugin();
        assert_eq!(*plugin.chain_id(), ChainId::Bitcoin);
    }

    #[test]
    fn factory_has_three_networks() {
        let plugin = bitcoin_plugin();
        assert_eq!(plugin.network_provider().networks().len(), 3);
    }

    #[test]
    fn native_asset_is_btc() {
        let plugin = bitcoin_plugin();
        assert_eq!(plugin.native_asset().symbol, "BTC");
        assert_eq!(plugin.native_asset().decimals, 8);
    }

    #[test]
    fn default_network_is_mainnet() {
        let plugin = bitcoin_plugin();
        let default = plugin.network_provider().default_network();
        assert_eq!(default.id.as_ref(), "bitcoin-mainnet");
    }
}

// Rust guideline compliant 2026-05-02
