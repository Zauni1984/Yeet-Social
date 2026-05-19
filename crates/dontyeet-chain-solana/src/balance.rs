//! Solana balance fetching via JSON-RPC API.
//!
//! Queries `getBalance` and returns the balance in lamports
//! (1 SOL = 1,000,000,000 lamports).

use std::collections::HashMap;

use async_trait::async_trait;
use url::Url;

use dontyeet_network::{Endpoints, HttpClient, ReqwestClient};
use dontyeet_primitives::address::Address;
use dontyeet_primitives::amount::Amount;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::BalanceFetcher;

/// SOL decimal places (1 SOL = 1,000,000,000 lamports).
const SOL_DECIMALS: u8 = 9;

/// JSON-RPC response for `getBalance`.
#[derive(Debug, serde::Deserialize)]
struct GetBalanceResponse {
    result: BalanceResult,
}

/// The `result` object within a `getBalance` response.
#[derive(Debug, serde::Deserialize)]
struct BalanceResult {
    value: u64,
}

/// Fetches Solana balances via the Solana JSON-RPC API.
pub struct SolBalanceFetcher {
    endpoints: Endpoints,
}

impl SolBalanceFetcher {
    /// Create a balance fetcher from the configured API URLs.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            endpoints: Endpoints::new(api_urls.clone()),
        }
    }
}

#[async_trait]
impl BalanceFetcher for SolBalanceFetcher {
    /// Fetch the native SOL balance for an address (in lamports).
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the API call fails, or
    /// `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn fetch_balance(&self, address: &Address, network: &NetworkId) -> Result<Amount> {
        let base = self.endpoints.primary(network)?;

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getBalance",
            "params": [address.as_str()]
        });

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let response = client
            .post_json(base, &body)
            .await
            .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

        let balance_resp: GetBalanceResponse = response
            .json()
            .map_err(|e| DontYeetWalletError::Network(format!("balance parse error: {e}")))?;

        Ok(Amount::from_raw(
            u128::from(balance_resp.result.value),
            SOL_DECIMALS,
        ))
    }
}

// Rust guideline compliant 2026-05-02
