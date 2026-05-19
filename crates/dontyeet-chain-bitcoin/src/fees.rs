//! Bitcoin fee estimation.
//!
//! Provides [`BtcFees`] and a [`FeeEstimator`] implementation that queries
//! the Mempool.space REST API for recommended fee rates.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use url::Url;

use dontyeet_network::{HttpClient, ReqwestClient};
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::FeeEstimator;
use dontyeet_primitives::transaction::FeeTier;

/// Bitcoin fee parameters for a transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BtcFees {
    /// Fee rate in satoshis per virtual byte.
    pub sats_per_vbyte: u64,
}

/// Mempool.space fee recommendation response.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_field_names,
    reason = "fields share the '_fee' suffix to mirror the mempool.space wire schema"
)]
struct MempoolFeeResponse {
    fastest_fee: u64,
    half_hour_fee: u64,
    #[expect(
        dead_code,
        reason = "deserialized from mempool.space wire response; kept to document the schema"
    )]
    hour_fee: u64,
    economy_fee: u64,
    #[expect(
        dead_code,
        reason = "deserialized from mempool.space wire response; kept to document the schema"
    )]
    minimum_fee: u64,
}

/// Estimates Bitcoin fees by querying Mempool.space `/v1/fees/recommended`.
pub struct BtcFeeEstimator {
    api_urls: HashMap<NetworkId, Vec<Url>>,
}

impl BtcFeeEstimator {
    /// Create a fee estimator from the configured API URLs.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            api_urls: api_urls.clone(),
        }
    }
}

#[async_trait]
impl FeeEstimator<BtcFees> for BtcFeeEstimator {
    /// Estimate slow / standard / fast fee tiers.
    ///
    /// Maps Mempool.space recommendations:
    /// - slow = `economyFee`
    /// - standard = `halfHourFee`
    /// - fast = `fastestFee`
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the API call fails, or
    /// `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn estimate_fees(&self, network: &NetworkId) -> Result<FeeTier<BtcFees>> {
        let urls = self
            .api_urls
            .get(network)
            .ok_or_else(|| DontYeetWalletError::NotFound(format!("no API URLs for {network}")))?;

        let base = urls
            .first()
            .ok_or_else(|| DontYeetWalletError::NotFound("API URL list is empty".into()))?;

        let fee_url = base
            .join("/v1/fees/recommended")
            .map_err(|e| DontYeetWalletError::Network(format!("URL join error: {e}")))?;

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let response = client
            .get(&fee_url)
            .await
            .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

        let rec: MempoolFeeResponse = response
            .json()
            .map_err(|e| DontYeetWalletError::Network(format!("fee response parse error: {e}")))?;

        Ok(FeeTier {
            slow: BtcFees {
                sats_per_vbyte: rec.economy_fee,
            },
            standard: BtcFees {
                sats_per_vbyte: rec.half_hour_fee,
            },
            fast: BtcFees {
                sats_per_vbyte: rec.fastest_fee,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn btc_fees_serialize_roundtrip() {
        let fees = BtcFees { sats_per_vbyte: 15 };
        let json = serde_json::to_string(&fees).expect("serialize");
        let parsed: BtcFees = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.sats_per_vbyte, 15);
    }

    #[test]
    fn fee_tier_serialize() {
        let tier = FeeTier {
            slow: BtcFees { sats_per_vbyte: 3 },
            standard: BtcFees { sats_per_vbyte: 10 },
            fast: BtcFees { sats_per_vbyte: 25 },
        };
        let json = serde_json::to_string(&tier).expect("serialize");
        assert!(json.contains("\"sats_per_vbyte\""));
    }
}

// Rust guideline compliant 2026-05-02
