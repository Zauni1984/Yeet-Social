//! EVM chain plugin — bundles all EVM capabilities into a single
//! [`ChainPlugin`] implementation.

use std::any::Any;

use async_trait::async_trait;

use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::secret::Seed;
use dontyeet_primitives::traits::{
    ChainPlugin, FeeEstimator, KeyDeriver, TransactionBroadcaster, TransactionBuilder,
    TransactionSigner,
};
use dontyeet_primitives::transaction::{
    RpcHealthResult, StandardizedFee, StandardizedFeeTier, TxHash,
};
use dontyeet_primitives::{Address, Amount, Result};

use dontyeet_chain::FnSigner;
use dontyeet_network::NetworkCatalog;

use crate::balance::EvmBalanceFetcher;
use crate::broadcast::EvmBroadcaster;
use crate::config::EvmChainConfig;
use crate::fees::EvmFeeEstimator;
use crate::history::EvmHistoryFetcher;
use crate::keys::{EvmAddressEncoder, EvmKeyDeriver};
use crate::token_balance::EvmTokenBalanceFetcher;
use crate::tx::EvmTransactionBuilder;

/// A complete EVM chain integration.
///
/// One instance per supported chain (Ethereum, Polygon, BNB, Avalanche,
/// Sonic).  Constructed via the factory functions in [`crate::chains`].
pub struct EvmChainPlugin {
    config: EvmChainConfig,
    key_deriver: EvmKeyDeriver,
    address_encoder: EvmAddressEncoder,
    fee_estimator: EvmFeeEstimator,
    balance_fetcher: EvmBalanceFetcher,
    tx_builder: EvmTransactionBuilder,
    tx_signer: FnSigner,
    broadcaster: EvmBroadcaster,
    network_catalog: NetworkCatalog,
    token_balance_fetcher: EvmTokenBalanceFetcher,
    history_fetcher: EvmHistoryFetcher,
}

impl EvmChainPlugin {
    /// Create a new EVM chain plugin from the given configuration.
    #[must_use]
    pub fn new(config: EvmChainConfig) -> Self {
        let key_deriver = EvmKeyDeriver::new(&config);
        let fee_estimator = EvmFeeEstimator::new(&config.rpc_urls);
        let balance_fetcher = EvmBalanceFetcher::new(&config.rpc_urls);
        let tx_builder = EvmTransactionBuilder::new(&config);
        let evm_chain_id = config.evm_chain_id_mainnet;
        let tx_signer = FnSigner::new(move |msg, key| {
            crate::signing::sign_legacy_tx(msg, key, evm_chain_id)
        });
        let broadcaster = EvmBroadcaster::new(&config.rpc_urls);
        let network_catalog = NetworkCatalog::new(
            config.networks.clone(),
            config.explorer_urls.clone(),
            config.rpc_urls.clone(),
        );
        let token_balance_fetcher = EvmTokenBalanceFetcher::new(&config.rpc_urls);
        let history_fetcher = EvmHistoryFetcher::new(
            &config.explorer_api_urls,
            &config.explorer_api_key,
            &config.native_asset.symbol,
        );

        Self {
            config,
            key_deriver,
            address_encoder: EvmAddressEncoder,
            fee_estimator,
            balance_fetcher,
            tx_builder,
            tx_signer,
            broadcaster,
            network_catalog,
            token_balance_fetcher,
            history_fetcher,
        }
    }

    /// Borrow the underlying configuration.
    #[must_use]
    pub fn config(&self) -> &EvmChainConfig {
        &self.config
    }
}

#[async_trait]
impl ChainPlugin for EvmChainPlugin {
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
        &self.tx_builder
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "elapsed health-check time fits in u64 ms; network timeouts cap it well below u128 saturation"
    )]
    async fn check_rpc_health(&self, network: &NetworkId) -> Result<RpcHealthResult> {
        let Some(urls) = self.config.rpc_urls.get(network) else {
            return Ok(RpcHealthResult {
                reachable: false,
                response_time_ms: 0,
                latest_block: None,
                error: Some(format!("no RPC URLs for {network}")),
            });
        };

        let start = std::time::Instant::now();
        let result: std::result::Result<String, _> =
            crate::rpc::rpc_call(urls, "eth_blockNumber", serde_json::json!([])).await;
        let elapsed = start.elapsed().as_millis() as u64;

        match result {
            Ok(hex_str) => match crate::rpc::parse_hex_u64(&hex_str) {
                Ok(block) => Ok(RpcHealthResult {
                    reachable: true,
                    response_time_ms: elapsed,
                    latest_block: Some(block),
                    error: None,
                }),
                Err(e) => Ok(RpcHealthResult {
                    reachable: true,
                    response_time_ms: elapsed,
                    latest_block: None,
                    error: Some(e.to_string()),
                }),
            },
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
        let symbol = &self.config.native_asset.symbol;
        let decimals = self.config.native_asset.decimals;

        let format_tier = |fees: &crate::fees::EvmFees| -> Result<StandardizedFee> {
            let gas_limit = u128::from(fees.gas_limit);
            let total_wei = gas_limit.checked_mul(fees.gas_price_wei).ok_or_else(|| {
                dontyeet_primitives::DontYeetWalletError::Chain("fee arithmetic overflow".into())
            })?;

            // Build a decimal string: total_wei / 10^decimals.
            let divisor = 10_u128.checked_pow(u32::from(decimals)).ok_or_else(|| {
                dontyeet_primitives::DontYeetWalletError::Chain("decimals overflow".into())
            })?;
            let whole = total_wei / divisor;
            let frac = total_wei % divisor;

            // Format fractional part with leading zeros up to `decimals` width,
            // then truncate to 6 significant fractional digits for readability.
            let frac_str = format!("{frac:0>width$}", width = usize::from(decimals));
            let display_digits = usize::from(decimals).min(6);
            let frac_trimmed = &frac_str[..display_digits];

            let label = format!("~{whole}.{frac_trimmed} {symbol}");
            let native_amount = total_wei.to_string();

            Ok(StandardizedFee {
                label,
                native_amount,
            })
        };

        Ok(StandardizedFeeTier {
            slow: format_tier(&tiers.slow)?,
            standard: format_tier(&tiers.standard)?,
            fast: format_tier(&tiers.fast)?,
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
        let private_key = keypair.private_key().ok_or_else(|| {
            dontyeet_primitives::DontYeetWalletError::Chain("no private key derived".into())
        })?;

        // 2. Estimate fees (use standard tier)
        let fee_tiers = self.fee_estimator.estimate_fees(network).await?;
        let fees = fee_tiers.standard;

        // 3. Build unsigned transaction
        let unsigned_tx = self
            .tx_builder
            .build_simple_transfer(&keypair, to, amount, &fees, network)
            .await?;

        // 4. Sign
        let signed_tx = self.tx_signer.sign(&unsigned_tx, private_key)?;

        // 5. Broadcast
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

// Rust guideline compliant 2026-05-02
