//! EVM fee estimation.
//!
//! Provides [`EvmFees`] and a [`FeeEstimator`] implementation that queries
//! `eth_gasPrice` from an RPC node.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use url::Url;

use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::FeeEstimator;
use dontyeet_primitives::transaction::FeeTier;

use crate::rpc;

/// Gas limit for a simple ETH/native-coin transfer.
pub const SIMPLE_TRANSFER_GAS: u64 = 21_000;

/// Hard cap on gas price: 500 gwei.  Any RPC response above this is
/// treated as malicious or erroneous and rejected outright.
const MAX_GAS_PRICE_WEI: u128 = 500_000_000_000;

/// Warning threshold: 100 gwei.  Prices above this are logged as a
/// warning but still allowed.
const WARN_GAS_PRICE_WEI: u128 = 100_000_000_000;

/// EVM fee parameters for a transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvmFees {
    /// Maximum gas units the transaction may consume.
    pub gas_limit: u64,
    /// Gas price in wei.
    pub gas_price_wei: u128,
}

/// Estimates EVM fees by querying `eth_gasPrice`.
pub struct EvmFeeEstimator {
    rpc_urls: HashMap<NetworkId, Vec<Url>>,
}

impl EvmFeeEstimator {
    /// Create a fee estimator from the configured RPC URLs.
    #[must_use]
    pub fn new(rpc_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            rpc_urls: rpc_urls.clone(),
        }
    }
}

#[async_trait]
impl FeeEstimator<EvmFees> for EvmFeeEstimator {
    /// Estimate slow / standard / fast fee tiers.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the RPC call fails, or
    /// `DontYeetWalletError::NotFound` if no RPC URLs exist for the network.
    async fn estimate_fees(&self, network: &NetworkId) -> Result<FeeTier<EvmFees>> {
        let urls = self
            .rpc_urls
            .get(network)
            .ok_or_else(|| DontYeetWalletError::NotFound(format!("no RPC URLs for {network}")))?;

        let gas_price_hex: String =
            rpc::rpc_call(urls, "eth_gasPrice", serde_json::json!([])).await?;

        let gas_price = rpc::parse_hex_u128(&gas_price_hex)?;

        // Validate gas price is within safe bounds.
        if gas_price == 0 {
            return Err(DontYeetWalletError::Validation(
                "RPC returned zero gas price".into(),
            ));
        }
        if gas_price > MAX_GAS_PRICE_WEI {
            return Err(DontYeetWalletError::Validation(format!(
                "gas price {gas_price} wei exceeds safety cap of {MAX_GAS_PRICE_WEI} wei (500 gwei)"
            )));
        }
        if gas_price > WARN_GAS_PRICE_WEI {
            tracing::warn!(
                gas_price_gwei = gas_price / 1_000_000_000,
                "gas price is unusually high"
            );
        }

        // Slow = 80% of current, Standard = current, Fast = 150%.
        let slow_price = gas_price
            .checked_mul(80)
            .and_then(|v| v.checked_div(100))
            .ok_or_else(|| DontYeetWalletError::Chain("fee arithmetic overflow".into()))?;

        let fast_price = gas_price
            .checked_mul(150)
            .and_then(|v| v.checked_div(100))
            .ok_or_else(|| DontYeetWalletError::Chain("fee arithmetic overflow".into()))?;

        Ok(FeeTier {
            slow: EvmFees {
                gas_limit: SIMPLE_TRANSFER_GAS,
                gas_price_wei: slow_price,
            },
            standard: EvmFees {
                gas_limit: SIMPLE_TRANSFER_GAS,
                gas_price_wei: gas_price,
            },
            fast: EvmFees {
                gas_limit: SIMPLE_TRANSFER_GAS,
                gas_price_wei: fast_price,
            },
        })
    }
}

// Rust guideline compliant 2026-05-02
