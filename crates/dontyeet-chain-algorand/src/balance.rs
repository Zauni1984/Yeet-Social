//! Algorand balance fetching via Algod REST v2 API.
//!
//! Queries `GET /v2/accounts/{addr}` and extracts the `amount` field.

use std::collections::HashMap;

use async_trait::async_trait;
use url::Url;

use dontyeet_network::{Endpoints, HttpClient, ReqwestClient, encode_segment};
use dontyeet_primitives::address::Address;
use dontyeet_primitives::amount::Amount;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::BalanceFetcher;

/// ALGO decimal places (1 ALGO = `1_000_000` `microAlgos`).
const ALGO_DECIMALS: u8 = 6;

/// Algod account info response (partial).
#[derive(Debug, serde::Deserialize)]
struct AccountInfo {
    amount: u64,
}

/// Fetches Algorand balances via the Algod REST v2 API.
pub struct AlgoBalanceFetcher {
    endpoints: Endpoints,
}

impl AlgoBalanceFetcher {
    /// Create a balance fetcher from the configured API URLs.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            endpoints: Endpoints::new(api_urls.clone()),
        }
    }
}

#[async_trait]
impl BalanceFetcher for AlgoBalanceFetcher {
    /// Fetch the on-chain ALGO balance for an address.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the API call fails, or
    /// `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn fetch_balance(&self, address: &Address, network: &NetworkId) -> Result<Amount> {
        let base = self.endpoints.primary(network)?;

        let path = format!("/v2/accounts/{}", encode_segment(address.as_str()));
        let account_url = base
            .join(&path)
            .map_err(|e| DontYeetWalletError::Network(format!("URL join error: {e}")))?;

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let response = client
            .get(&account_url)
            .await
            .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

        let info: AccountInfo = response
            .json()
            .map_err(|e| DontYeetWalletError::Network(format!("account info parse error: {e}")))?;

        Ok(Amount::from_raw(u128::from(info.amount), ALGO_DECIMALS))
    }
}

// Rust guideline compliant 2026-05-02
