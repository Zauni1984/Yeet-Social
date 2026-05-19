//! XRP Ledger chain plugin — bundles all XRP capabilities into a single
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

use crate::balance::XrpBalanceFetcher;
use crate::broadcast::XrpBroadcaster;
use crate::config::{XrpConfig, default_xrp_config};
use crate::fees::XrpFeeEstimator;
use crate::history::XrpHistoryFetcher;
use crate::keys::{XrpAddressEncoder, XrpKeyDeriver};

/// A complete XRP Ledger chain integration.
///
/// Handles mainnet and testnet. Uses secp256k1 keys with XRP's custom
/// Base58 address encoding and the XRP Ledger JSON-RPC API for all
/// network operations.
pub struct XrpChainPlugin {
    config: XrpConfig,
    key_deriver: XrpKeyDeriver,
    address_encoder: XrpAddressEncoder,
    fee_estimator: XrpFeeEstimator,
    balance_fetcher: XrpBalanceFetcher,
    tx_signer: FnSigner,
    broadcaster: XrpBroadcaster,
    network_catalog: NetworkCatalog,
    history_fetcher: XrpHistoryFetcher,
}

impl XrpChainPlugin {
    /// Create a new XRP chain plugin from the given configuration.
    #[must_use]
    pub fn new(config: XrpConfig) -> Self {
        let key_deriver = XrpKeyDeriver::new(&config);
        let fee_estimator = XrpFeeEstimator::new(&config.api_urls);
        let balance_fetcher = XrpBalanceFetcher::new(&config.api_urls);
        let tx_signer = FnSigner::new(crate::signing::sign_unsigned_payload);
        let broadcaster = XrpBroadcaster::new(&config.api_urls);
        let network_catalog = NetworkCatalog::new(
            config.networks.clone(),
            config.explorer_urls.clone(),
            config.api_urls.clone(),
        );
        let history_fetcher = XrpHistoryFetcher::new(&config.api_urls);

        Self {
            config,
            key_deriver,
            address_encoder: XrpAddressEncoder,
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
    pub fn config(&self) -> &XrpConfig {
        &self.config
    }
}

#[async_trait]
impl ChainPlugin for XrpChainPlugin {
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
        // XRP transaction building is simpler than UTXO chains but
        // still deferred to a future iteration. Placeholder.
        self
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "elapsed health-check time fits in u64 ms; network timeouts cap it well below u128 saturation"
    )]
    async fn check_rpc_health(&self, network: &NetworkId) -> Result<RpcHealthResult> {
        #[derive(serde::Deserialize)]
        struct ValidatedLedger {
            seq: u64,
        }
        #[derive(serde::Deserialize)]
        struct ServerInfo {
            validated_ledger: Option<ValidatedLedger>,
        }
        #[derive(serde::Deserialize)]
        struct InfoResult {
            info: ServerInfo,
        }
        #[derive(serde::Deserialize)]
        struct ServerInfoResponse {
            result: InfoResult,
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

        let body = serde_json::json!({
            "method": "server_info",
            "params": [{}]
        });

        let start = std::time::Instant::now();
        let result = async {
            let client =
                ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
            let response = client
                .post_json(base, &body)
                .await
                .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
            let resp: ServerInfoResponse = response
                .json()
                .map_err(|e| DontYeetWalletError::Network(format!("server_info parse error: {e}")))?;
            Ok::<Option<u64>, DontYeetWalletError>(resp.result.info.validated_ledger.map(|vl| vl.seq))
        }
        .await;
        let elapsed = start.elapsed().as_millis() as u64;

        match result {
            Ok(seq) => Ok(RpcHealthResult {
                reachable: true,
                response_time_ms: elapsed,
                latest_block: seq,
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
        let tiers = self.fee_estimator.estimate_fees(network).await?;

        let xrp_label = |drops: u64| -> String {
            let whole = drops / 1_000_000;
            let frac = drops % 1_000_000;
            format!("~{whole}.{frac:06} XRP")
        };

        Ok(StandardizedFeeTier {
            slow: StandardizedFee {
                label: xrp_label(tiers.slow.drops),
                native_amount: tiers.slow.drops.to_string(),
            },
            standard: StandardizedFee {
                label: xrp_label(tiers.standard.drops),
                native_amount: tiers.standard.drops.to_string(),
            },
            fast: StandardizedFee {
                label: xrp_label(tiers.fast.drops),
                native_amount: tiers.fast.drops.to_string(),
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

/// Create an [`XrpChainPlugin`] with the default configuration.
///
/// Includes mainnet and testnet networks with XRP Ledger JSON-RPC
/// API endpoints.
#[must_use]
pub fn xrp_plugin() -> XrpChainPlugin {
    XrpChainPlugin::new(default_xrp_config())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dontyeet_primitives::chain::ChainId;

    #[test]
    fn factory_produces_xrp_chain_id() {
        let plugin = xrp_plugin();
        assert_eq!(*plugin.chain_id(), ChainId::Xrp);
    }

    #[test]
    fn factory_has_two_networks() {
        let plugin = xrp_plugin();
        assert_eq!(plugin.network_provider().networks().len(), 2);
    }

    #[test]
    fn native_asset_is_xrp() {
        let plugin = xrp_plugin();
        assert_eq!(plugin.native_asset().symbol, "XRP");
        assert_eq!(plugin.native_asset().decimals, 6);
    }

    #[test]
    fn default_network_is_mainnet() {
        let plugin = xrp_plugin();
        let default = plugin.network_provider().default_network();
        assert_eq!(default.id.as_ref(), "xrp-mainnet");
    }
}

// Rust guideline compliant 2026-05-02
