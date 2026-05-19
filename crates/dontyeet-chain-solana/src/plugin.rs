//! Solana chain plugin — bundles all Solana capabilities into a single
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

use crate::balance::SolBalanceFetcher;
use crate::broadcast::SolBroadcaster;
use crate::config::{SolConfig, default_sol_config};
use crate::fees::SolFeeEstimator;
use crate::history::SolHistoryFetcher;
use crate::keys::{SolAddressEncoder, SolKeyDeriver};
use crate::token_balance::SolTokenBalanceFetcher;

/// A complete Solana chain integration.
///
/// Handles mainnet, devnet, and testnet. Uses Ed25519 keys with
/// Base58-encoded addresses and the Solana JSON-RPC API for all
/// network operations.
pub struct SolChainPlugin {
    config: SolConfig,
    key_deriver: SolKeyDeriver,
    address_encoder: SolAddressEncoder,
    fee_estimator: SolFeeEstimator,
    balance_fetcher: SolBalanceFetcher,
    tx_signer: FnSigner,
    broadcaster: SolBroadcaster,
    network_catalog: NetworkCatalog,
    token_balance_fetcher: SolTokenBalanceFetcher,
    history_fetcher: SolHistoryFetcher,
}

impl SolChainPlugin {
    /// Create a new Solana chain plugin from the given configuration.
    #[must_use]
    pub fn new(config: SolConfig) -> Self {
        let key_deriver = SolKeyDeriver::new(&config);
        let fee_estimator = SolFeeEstimator::new(&config.api_urls);
        let balance_fetcher = SolBalanceFetcher::new(&config.api_urls);
        let tx_signer = FnSigner::new(|msg, key| {
            crate::signing::sign_message(msg, key).map(|sig| sig.to_vec())
        });
        let broadcaster = SolBroadcaster::new(&config.api_urls);
        let network_catalog = NetworkCatalog::new(
            config.networks.clone(),
            config.explorer_urls.clone(),
            config.api_urls.clone(),
        );
        let token_balance_fetcher = SolTokenBalanceFetcher::new(&config.api_urls);
        let history_fetcher = SolHistoryFetcher::new(&config.api_urls);

        Self {
            config,
            key_deriver,
            address_encoder: SolAddressEncoder,
            fee_estimator,
            balance_fetcher,
            tx_signer,
            broadcaster,
            network_catalog,
            token_balance_fetcher,
            history_fetcher,
        }
    }

    /// Borrow the underlying configuration.
    #[must_use]
    pub fn config(&self) -> &SolConfig {
        &self.config
    }
}

#[async_trait]
impl ChainPlugin for SolChainPlugin {
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
        // Solana transaction building (system program, SPL) is complex.
        // Placeholder — returns self as Any for future downcasting.
        self
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "elapsed health-check time fits in u64 ms; network timeouts cap it well below u128 saturation"
    )]
    async fn check_rpc_health(&self, network: &NetworkId) -> Result<RpcHealthResult> {
        #[derive(serde::Deserialize)]
        struct SlotResponse {
            result: u64,
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
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getSlot",
            "params": [{"commitment": "finalized"}]
        });

        let start = std::time::Instant::now();
        let result = async {
            let client =
                ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
            let response = client
                .post_json(base, &body)
                .await
                .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
            let slot_resp: SlotResponse = response
                .json()
                .map_err(|e| DontYeetWalletError::Network(format!("slot parse error: {e}")))?;
            Ok::<u64, DontYeetWalletError>(slot_resp.result)
        }
        .await;
        let elapsed = start.elapsed().as_millis() as u64;

        match result {
            Ok(slot) => Ok(RpcHealthResult {
                reachable: true,
                response_time_ms: elapsed,
                latest_block: Some(slot),
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
        const DEFAULT_CU: u64 = 200_000;
        const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

        let tiers = self.fee_estimator.estimate_fees(network).await?;

        let to_display = |fees: &crate::fees::SolanaFees| -> StandardizedFee {
            // priority_fee is in micro-lamports per CU.
            // total priority lamports = micro_lamports * CU / 1_000_000
            let priority_lamports =
                fees.priority_fee_micro_lamports.saturating_mul(DEFAULT_CU) / 1_000_000;
            let total_lamports = fees
                .lamports_per_signature
                .saturating_add(priority_lamports);

            #[expect(
                clippy::cast_precision_loss,
                reason = "display-only conversion of lamport count to f64 SOL; loss is acceptable in human-readable label"
            )]
            let sol = total_lamports as f64 / LAMPORTS_PER_SOL;

            StandardizedFee {
                label: format!("~{sol:.6} SOL"),
                native_amount: total_lamports.to_string(),
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
        // 1. Derive keys
        let keypair = self.key_deriver.derive_keypair(seed, network)?;
        let private_key = keypair
            .private_key()
            .ok_or_else(|| DontYeetWalletError::Chain("no private key derived".into()))?;

        // 2. Build and sign the transfer transaction
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

    async fn fetch_token_balance(
        &self,
        address: &Address,
        contract: &str,
        network: &NetworkId,
    ) -> Result<(Amount, String)> {
        self.token_balance_fetcher
            .fetch_balance(address.as_str(), contract, network)
            .await
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

/// Create a [`SolChainPlugin`] with the default configuration.
///
/// Includes mainnet, devnet, and testnet networks with public Solana
/// JSON-RPC endpoints.
#[must_use]
pub fn solana_plugin() -> SolChainPlugin {
    SolChainPlugin::new(default_sol_config())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dontyeet_primitives::chain::ChainId;

    #[test]
    fn factory_produces_solana_chain_id() {
        let plugin = solana_plugin();
        assert_eq!(*plugin.chain_id(), ChainId::Solana);
    }

    #[test]
    fn factory_has_three_networks() {
        let plugin = solana_plugin();
        assert_eq!(plugin.network_provider().networks().len(), 3);
    }

    #[test]
    fn native_asset_is_sol() {
        let plugin = solana_plugin();
        assert_eq!(plugin.native_asset().symbol, "SOL");
        assert_eq!(plugin.native_asset().decimals, 9);
    }

    #[test]
    fn default_network_is_mainnet() {
        let plugin = solana_plugin();
        let default = plugin.network_provider().default_network();
        assert_eq!(default.id.as_ref(), "solana-mainnet");
    }
}

// Rust guideline compliant 2026-05-02
