//! XRP Ledger fee estimation.
//!
//! Provides [`XrpFees`] and a [`FeeEstimator`] implementation that queries
//! the XRP Ledger JSON-RPC `fee` method.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use url::Url;

use dontyeet_network::{HttpClient, ReqwestClient};
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::FeeEstimator;
use dontyeet_primitives::transaction::FeeTier;

/// XRP fee parameters for a transaction.
///
/// Fees are denominated in **drops** (1 XRP = 1,000,000 drops).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrpFees {
    /// Fee in drops (1 XRP = 1,000,000 drops).
    pub drops: u64,
}

/// JSON-RPC response for the XRP `fee` method.
#[derive(Debug, Deserialize)]
struct FeeRpcResponse {
    result: FeeResult,
}

/// Inner result of the fee RPC response.
#[derive(Debug, Deserialize)]
struct FeeResult {
    drops: FeeDrops,
}

/// Fee drop values from the XRP `fee` response.
#[derive(Debug, Deserialize)]
#[expect(
    clippy::struct_field_names,
    reason = "fields share '_fee' suffix to mirror the XRPL fee response schema"
)]
struct FeeDrops {
    base_fee: String,
    #[expect(
        dead_code,
        reason = "deserialized from XRPL fee response; kept to document the schema"
    )]
    median_fee: String,
    #[expect(
        dead_code,
        reason = "deserialized from XRPL fee response; kept to document the schema"
    )]
    minimum_fee: String,
    open_ledger_fee: String,
}

/// Estimates XRP fees by querying the JSON-RPC `fee` method.
pub struct XrpFeeEstimator {
    api_urls: HashMap<NetworkId, Vec<Url>>,
}

impl XrpFeeEstimator {
    /// Create a fee estimator from the configured API URLs.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            api_urls: api_urls.clone(),
        }
    }
}

#[async_trait]
impl FeeEstimator<XrpFees> for XrpFeeEstimator {
    /// Estimate slow / standard / fast fee tiers.
    ///
    /// Maps XRP fee response:
    /// - slow = `base_fee` (minimum accepted)
    /// - standard = `base_fee` (most transactions use base fee)
    /// - fast = `open_ledger_fee` (guaranteed next ledger)
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the API call fails, or
    /// `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn estimate_fees(&self, network: &NetworkId) -> Result<FeeTier<XrpFees>> {
        let urls = self
            .api_urls
            .get(network)
            .ok_or_else(|| DontYeetWalletError::NotFound(format!("no API URLs for {network}")))?;

        let base = urls
            .first()
            .ok_or_else(|| DontYeetWalletError::NotFound("API URL list is empty".into()))?;

        let body = serde_json::json!({
            "method": "fee",
            "params": [{}]
        });

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let response = client
            .post_json(base, &body)
            .await
            .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

        let fee_resp: FeeRpcResponse = response
            .json()
            .map_err(|e| DontYeetWalletError::Network(format!("fee response parse error: {e}")))?;

        let base_fee: u64 = fee_resp
            .result
            .drops
            .base_fee
            .parse()
            .map_err(|e| DontYeetWalletError::Network(format!("base_fee parse error: {e}")))?;

        let open_fee: u64 = fee_resp
            .result
            .drops
            .open_ledger_fee
            .parse()
            .map_err(|e| DontYeetWalletError::Network(format!("open_ledger_fee parse: {e}")))?;

        Ok(FeeTier {
            slow: XrpFees { drops: base_fee },
            standard: XrpFees { drops: base_fee },
            fast: XrpFees { drops: open_fee },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xrp_fees_serialize_roundtrip() {
        let fees = XrpFees { drops: 12 };
        let json = serde_json::to_string(&fees).expect("serialize");
        let parsed: XrpFees = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.drops, 12);
    }

    #[test]
    fn fee_tier_serialize() {
        let tier = FeeTier {
            slow: XrpFees { drops: 10 },
            standard: XrpFees { drops: 12 },
            fast: XrpFees { drops: 50 },
        };
        let json = serde_json::to_string(&tier).expect("serialize");
        assert!(json.contains("\"drops\""));
    }

    #[test]
    fn xrp_fees_deserialize_from_json() {
        let json = r#"{"drops":42}"#;
        let fees: XrpFees = serde_json::from_str(json).expect("deserialize");
        assert_eq!(fees.drops, 42);
    }
}

// Rust guideline compliant 2026-05-02
