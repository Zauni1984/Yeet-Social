//! Cardano fee estimation.
//!
//! Provides [`CardanoFees`] and a [`FeeEstimator`] implementation
//! that queries Blockfrost for protocol parameters.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use url::Url;

use dontyeet_network::{HttpClient, ReqwestClient};
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::FeeEstimator;
use dontyeet_primitives::transaction::FeeTier;

/// Cardano fee parameters for a transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardanoFees {
    /// Fee in lovelace (1 ADA = 1,000,000 lovelace).
    pub lovelace: u64,
}

/// Blockfrost protocol parameters response (partial).
#[derive(Debug, Deserialize)]
struct ProtocolParams {
    min_fee_a: u64,
    min_fee_b: u64,
}

/// Estimates Cardano fees by querying Blockfrost
/// `/epochs/latest/parameters`.
pub struct CardanoFeeEstimator {
    api_urls: HashMap<NetworkId, Vec<Url>>,
    project_id: Option<String>,
}

impl CardanoFeeEstimator {
    /// Create a fee estimator from the configured API URLs and `project_id`.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>, project_id: Option<String>) -> Self {
        Self {
            api_urls: api_urls.clone(),
            project_id,
        }
    }
}

/// Typical simple transaction size in bytes (for fee estimation).
const TYPICAL_TX_SIZE: u64 = 300;

#[async_trait]
impl FeeEstimator<CardanoFees> for CardanoFeeEstimator {
    /// Estimate slow / standard / fast fee tiers.
    ///
    /// Cardano fee formula: `min_fee_a × tx_size + min_fee_b`.
    /// We estimate using a typical transfer size of 300 bytes.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the API call fails, or
    /// `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn estimate_fees(&self, network: &NetworkId) -> Result<FeeTier<CardanoFees>> {
        let urls = self
            .api_urls
            .get(network)
            .ok_or_else(|| DontYeetWalletError::NotFound(format!("no API URLs for {network}")))?;

        let base = urls
            .first()
            .ok_or_else(|| DontYeetWalletError::NotFound("API URL list is empty".into()))?;

        let params_url = base
            .join("/epochs/latest/parameters")
            .map_err(|e| DontYeetWalletError::Network(format!("URL join error: {e}")))?;

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let headers = crate::auth::project_id_headers(self.project_id.as_deref());
        let response = client
            .get_with_headers(&params_url, &headers)
            .await
            .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

        let params: ProtocolParams = response
            .json()
            .map_err(|e| DontYeetWalletError::Network(format!("params parse error: {e}")))?;

        // fee = min_fee_a * tx_size + min_fee_b
        let base_fee = params
            .min_fee_a
            .saturating_mul(TYPICAL_TX_SIZE)
            .saturating_add(params.min_fee_b);

        Ok(FeeTier {
            slow: CardanoFees { lovelace: base_fee },
            standard: CardanoFees {
                lovelace: base_fee.saturating_mul(120) / 100,
            },
            fast: CardanoFees {
                lovelace: base_fee.saturating_mul(150) / 100,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cardano_fees_serialize_roundtrip() {
        let fees = CardanoFees { lovelace: 170_000 };
        let json = serde_json::to_string(&fees).expect("serialize");
        let parsed: CardanoFees = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.lovelace, 170_000);
    }

    #[test]
    fn fee_tier_serialize() {
        let tier = FeeTier {
            slow: CardanoFees { lovelace: 170_000 },
            standard: CardanoFees { lovelace: 204_000 },
            fast: CardanoFees { lovelace: 255_000 },
        };
        let json = serde_json::to_string(&tier).expect("serialize");
        assert!(json.contains("\"lovelace\""));
    }
}

// Rust guideline compliant 2026-05-02
