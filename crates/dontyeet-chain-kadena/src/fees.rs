//! Kadena fee estimation.
//!
//! Kadena uses a gas-based fee model where transactions specify a gas
//! limit and gas price.  The total fee is `gas_limit * gas_price` in KDA.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use url::Url;

use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::FeeEstimator;
use dontyeet_primitives::transaction::FeeTier;

/// Default gas limit for a simple KDA transfer.
pub const SIMPLE_TRANSFER_GAS_LIMIT: u64 = 600;

/// Kadena fee parameters for a transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KadenaFees {
    /// Gas price in KDA (e.g. 1e-8 = 0.00000001 KDA).
    pub gas_price: f64,
    /// Gas limit for this transaction.
    pub gas_limit: u64,
}

impl KadenaFees {
    /// Total fee in KDA as a float.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "gas_limit fits well within f64's 2^53 integer range"
    )]
    pub fn total_kda(&self) -> f64 {
        self.gas_price * self.gas_limit as f64
    }
}

/// Estimates Kadena fees.
pub struct KadenaFeeEstimator {
    api_urls: HashMap<NetworkId, Vec<Url>>,
}

impl KadenaFeeEstimator {
    /// Create a fee estimator from the configured API URLs.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            api_urls: api_urls.clone(),
        }
    }
}

#[async_trait]
impl FeeEstimator<KadenaFees> for KadenaFeeEstimator {
    /// Estimate slow / standard / fast fee tiers for Kadena.
    ///
    /// Kadena's minimum gas price is 1e-8 KDA.  The tiers apply
    /// multipliers for faster inclusion.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn estimate_fees(&self, network: &NetworkId) -> Result<FeeTier<KadenaFees>> {
        let _urls = self
            .api_urls
            .get(network)
            .ok_or_else(|| DontYeetWalletError::NotFound(format!("no API URLs for {network}")))?;

        Ok(FeeTier {
            slow: KadenaFees {
                gas_price: 1e-8,
                gas_limit: SIMPLE_TRANSFER_GAS_LIMIT,
            },
            standard: KadenaFees {
                gas_price: 1e-7,
                gas_limit: SIMPLE_TRANSFER_GAS_LIMIT,
            },
            fast: KadenaFees {
                gas_price: 1e-6,
                gas_limit: SIMPLE_TRANSFER_GAS_LIMIT,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_kda_simple() {
        let fees = KadenaFees {
            gas_price: 1e-8,
            gas_limit: SIMPLE_TRANSFER_GAS_LIMIT,
        };
        let total = fees.total_kda();
        assert!(total > 0.0);
        assert!(total < 0.001); // A simple transfer should be cheap.
    }

    #[test]
    fn fees_serialize_roundtrip() {
        let fees = KadenaFees {
            gas_price: 1e-7,
            gas_limit: SIMPLE_TRANSFER_GAS_LIMIT,
        };
        let json = serde_json::to_string(&fees).expect("serialize");
        let decoded: KadenaFees = serde_json::from_str(&json).expect("deserialize");
        assert!((decoded.gas_price - 1e-7).abs() < f64::EPSILON);
        assert_eq!(decoded.gas_limit, SIMPLE_TRANSFER_GAS_LIMIT);
    }
}

// Rust guideline compliant 2026-05-02
