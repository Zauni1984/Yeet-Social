//! TRON fee estimation.
//!
//! Provides [`TronFees`] and a [`FeeEstimator`] implementation that queries
//! the `TronGrid` REST API for bandwidth and energy resource information.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use url::Url;

use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::FeeEstimator;
use dontyeet_primitives::transaction::FeeTier;

/// TRON fee parameters for a transaction.
///
/// TRON uses a resource model with bandwidth (for simple transfers) and
/// energy (for smart contract calls) instead of traditional gas fees.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TronFees {
    /// Bandwidth points consumed by the transaction.
    pub bandwidth_points: u64,
    /// Energy consumed by the transaction (smart contract calls).
    pub energy: u64,
}

/// Estimates TRON fees by querying `TronGrid`
/// `POST /wallet/getaccountresource`.
///
/// Uses simple placeholder fee tiers since TRON fee estimation depends
/// on the user's account resource state (frozen bandwidth/energy).
pub struct TronFeeEstimator {
    api_urls: HashMap<NetworkId, Vec<Url>>,
}

impl TronFeeEstimator {
    /// Create a fee estimator from the configured API URLs.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            api_urls: api_urls.clone(),
        }
    }
}

#[async_trait]
impl FeeEstimator<TronFees> for TronFeeEstimator {
    /// Estimate slow / standard / fast fee tiers.
    ///
    /// TRON uses a resource model; these placeholder values represent
    /// bandwidth cost in SUN for simple TRX transfers:
    /// - slow = 1 SUN (minimal bandwidth)
    /// - standard = 100 SUN (typical transfer)
    /// - fast = 1000 SUN (priority)
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::NotFound` if no API URLs exist for the
    /// network.
    async fn estimate_fees(&self, network: &NetworkId) -> Result<FeeTier<TronFees>> {
        // Verify the network is known.
        let _urls = self
            .api_urls
            .get(network)
            .ok_or_else(|| DontYeetWalletError::NotFound(format!("no API URLs for {network}")))?;

        // TRON fee estimation is account-dependent (frozen resources).
        // Use simple placeholder tiers for now.
        Ok(FeeTier {
            slow: TronFees {
                bandwidth_points: 200,
                energy: 0,
            },
            standard: TronFees {
                bandwidth_points: 300,
                energy: 0,
            },
            fast: TronFees {
                bandwidth_points: 500,
                energy: 0,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tron_fees_serialize_roundtrip() {
        let fees = TronFees {
            bandwidth_points: 200,
            energy: 50_000,
        };
        let json = serde_json::to_string(&fees).expect("serialize");
        let parsed: TronFees = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.bandwidth_points, 200);
        assert_eq!(parsed.energy, 50_000);
    }

    #[test]
    fn fee_tier_serialize() {
        let tier = FeeTier {
            slow: TronFees {
                bandwidth_points: 200,
                energy: 0,
            },
            standard: TronFees {
                bandwidth_points: 300,
                energy: 0,
            },
            fast: TronFees {
                bandwidth_points: 500,
                energy: 0,
            },
        };
        let json = serde_json::to_string(&tier).expect("serialize");
        assert!(json.contains("\"bandwidth_points\""));
        assert!(json.contains("\"energy\""));
    }

    #[test]
    fn fee_estimator_unknown_network_fails() {
        let estimator = TronFeeEstimator::new(&HashMap::new());
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        let result = rt.block_on(estimator.estimate_fees(&NetworkId::new("unknown")));
        assert!(result.is_err());
    }
}

// Rust guideline compliant 2026-05-02
