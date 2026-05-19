//! Solana fee estimation.
//!
//! Provides [`SolanaFees`] and a [`FeeEstimator`] implementation that queries
//! the Solana JSON-RPC API for recent prioritization fees.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use url::Url;

use dontyeet_network::{HttpClient, ReqwestClient};
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::FeeEstimator;
use dontyeet_primitives::transaction::FeeTier;

/// Base fee per signature in lamports (Solana constant).
const BASE_LAMPORTS_PER_SIGNATURE: u64 = 5000;

/// Solana fee parameters for a transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaFees {
    /// Base fee in lamports per signature.
    pub lamports_per_signature: u64,
    /// Priority fee in micro-lamports per compute unit.
    pub priority_fee_micro_lamports: u64,
}

/// A single prioritization fee entry from the RPC response.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrioritizationFeeEntry {
    prioritization_fee: u64,
}

/// Wraps the JSON-RPC response envelope.
#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    result: T,
}

/// Estimates Solana fees by querying `getRecentPrioritizationFees`.
pub struct SolFeeEstimator {
    api_urls: HashMap<NetworkId, Vec<Url>>,
}

impl SolFeeEstimator {
    /// Create a fee estimator from the configured API URLs.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            api_urls: api_urls.clone(),
        }
    }
}

#[async_trait]
impl FeeEstimator<SolanaFees> for SolFeeEstimator {
    /// Estimate slow / standard / fast fee tiers.
    ///
    /// Queries `getRecentPrioritizationFees` and derives percentile-based
    /// tiers. Falls back to zero priority fee if the RPC call fails.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the API call fails, or
    /// `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn estimate_fees(&self, network: &NetworkId) -> Result<FeeTier<SolanaFees>> {
        let urls = self
            .api_urls
            .get(network)
            .ok_or_else(|| DontYeetWalletError::NotFound(format!("no API URLs for {network}")))?;

        let base = urls
            .first()
            .ok_or_else(|| DontYeetWalletError::NotFound("API URL list is empty".into()))?;

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getRecentPrioritizationFees",
            "params": []
        });

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let response = client
            .post_json(base, &body)
            .await
            .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

        let rpc_resp: RpcResponse<Vec<PrioritizationFeeEntry>> = response
            .json()
            .map_err(|e| DontYeetWalletError::Network(format!("fee response parse error: {e}")))?;

        let priority = compute_priority_tiers(&rpc_resp.result);

        Ok(FeeTier {
            slow: SolanaFees {
                lamports_per_signature: BASE_LAMPORTS_PER_SIGNATURE,
                priority_fee_micro_lamports: priority.0,
            },
            standard: SolanaFees {
                lamports_per_signature: BASE_LAMPORTS_PER_SIGNATURE,
                priority_fee_micro_lamports: priority.1,
            },
            fast: SolanaFees {
                lamports_per_signature: BASE_LAMPORTS_PER_SIGNATURE,
                priority_fee_micro_lamports: priority.2,
            },
        })
    }
}

/// Compute (slow, standard, fast) priority fee from recent entries.
///
/// Returns percentile-based values: p25, p50, p75. Falls back to
/// `(0, 0, 0)` if the list is empty.
fn compute_priority_tiers(entries: &[PrioritizationFeeEntry]) -> (u64, u64, u64) {
    if entries.is_empty() {
        return (0, 0, 0);
    }

    let mut fees: Vec<u64> = entries.iter().map(|e| e.prioritization_fee).collect();
    fees.sort_unstable();

    let p25 = fees[fees.len() / 4];
    let p50 = fees[fees.len() / 2];
    let p75 = fees[fees.len() * 3 / 4];

    (p25, p50, p75)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solana_fees_serialize_roundtrip() {
        let fees = SolanaFees {
            lamports_per_signature: 5000,
            priority_fee_micro_lamports: 100,
        };
        let json = serde_json::to_string(&fees).expect("serialize");
        let parsed: SolanaFees = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.lamports_per_signature, 5000);
        assert_eq!(parsed.priority_fee_micro_lamports, 100);
    }

    #[test]
    fn fee_tier_serialize() {
        let tier = FeeTier {
            slow: SolanaFees {
                lamports_per_signature: 5000,
                priority_fee_micro_lamports: 10,
            },
            standard: SolanaFees {
                lamports_per_signature: 5000,
                priority_fee_micro_lamports: 50,
            },
            fast: SolanaFees {
                lamports_per_signature: 5000,
                priority_fee_micro_lamports: 200,
            },
        };
        let json = serde_json::to_string(&tier).expect("serialize");
        assert!(json.contains("\"lamports_per_signature\""));
        assert!(json.contains("\"priority_fee_micro_lamports\""));
    }

    #[test]
    fn compute_priority_empty_returns_zeros() {
        let result = compute_priority_tiers(&[]);
        assert_eq!(result, (0, 0, 0));
    }

    #[test]
    fn compute_priority_sorted() {
        let entries: Vec<PrioritizationFeeEntry> = (0..100)
            .map(|i| PrioritizationFeeEntry {
                prioritization_fee: i * 10,
            })
            .collect();
        let (slow, standard, fast) = compute_priority_tiers(&entries);
        assert!(slow <= standard);
        assert!(standard <= fast);
    }
}

// Rust guideline compliant 2026-05-02
