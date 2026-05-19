//! Algorand fee estimation.
//!
//! Provides [`AlgoFees`] and a [`FeeEstimator`] implementation that
//! queries the Algod REST v2 API for suggested transaction parameters.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use url::Url;

use dontyeet_network::{HttpClient, ReqwestClient};
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::FeeEstimator;
use dontyeet_primitives::transaction::FeeTier;

/// Algorand fee parameters for a transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlgoFees {
    /// Fee in `microAlgos` (1 ALGO = 1,000,000 `microAlgos`).
    pub micro_algos: u64,
}

/// Algod suggested transaction parameters response (partial).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct SuggestedParams {
    /// Minimum fee in `microAlgos`.
    min_fee: u64,
}

/// Estimates Algorand fees by querying Algod `/v2/transactions/params`.
pub struct AlgoFeeEstimator {
    api_urls: HashMap<NetworkId, Vec<Url>>,
}

impl AlgoFeeEstimator {
    /// Create a fee estimator from the configured API URLs.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            api_urls: api_urls.clone(),
        }
    }
}

#[async_trait]
impl FeeEstimator<AlgoFees> for AlgoFeeEstimator {
    /// Estimate slow / standard / fast fee tiers.
    ///
    /// Algorand has a flat minimum fee (currently 1,000 `microAlgos`).
    /// Tiers are set as multiples of the minimum for prioritization.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the API call fails, or
    /// `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn estimate_fees(&self, network: &NetworkId) -> Result<FeeTier<AlgoFees>> {
        let urls = self
            .api_urls
            .get(network)
            .ok_or_else(|| DontYeetWalletError::NotFound(format!("no API URLs for {network}")))?;

        let base = urls
            .first()
            .ok_or_else(|| DontYeetWalletError::NotFound("API URL list is empty".into()))?;

        let params_url = base
            .join("/v2/transactions/params")
            .map_err(|e| DontYeetWalletError::Network(format!("URL join error: {e}")))?;

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let response = client
            .get(&params_url)
            .await
            .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

        let params: SuggestedParams = response
            .json()
            .map_err(|e| DontYeetWalletError::Network(format!("params parse error: {e}")))?;

        Ok(FeeTier {
            slow: AlgoFees {
                micro_algos: params.min_fee,
            },
            standard: AlgoFees {
                micro_algos: params.min_fee.saturating_mul(2),
            },
            fast: AlgoFees {
                micro_algos: params.min_fee.saturating_mul(5),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn algo_fees_serialize_roundtrip() {
        let fees = AlgoFees { micro_algos: 1000 };
        let json = serde_json::to_string(&fees).expect("serialize");
        let parsed: AlgoFees = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.micro_algos, 1000);
    }

    #[test]
    fn fee_tier_serialize() {
        let tier = FeeTier {
            slow: AlgoFees { micro_algos: 1000 },
            standard: AlgoFees { micro_algos: 2000 },
            fast: AlgoFees { micro_algos: 5000 },
        };
        let json = serde_json::to_string(&tier).expect("serialize");
        assert!(json.contains("\"micro_algos\""));
    }
}

// Rust guideline compliant 2026-05-02
