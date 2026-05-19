//! TRON balance fetching via `TronGrid` REST API.
//!
//! Queries `POST /wallet/getaccount` with the address in Base58 format
//! and parses the `balance` field (in SUN, 1 TRX = 1,000,000 SUN).

use std::collections::HashMap;

use async_trait::async_trait;
use url::Url;

use dontyeet_network::{Endpoints, HttpClient, ReqwestClient};
use dontyeet_primitives::address::Address;
use dontyeet_primitives::amount::Amount;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::BalanceFetcher;

/// TRX decimal places (1 TRX = 1,000,000 SUN).
const TRX_DECIMALS: u8 = 6;

/// `TronGrid` account response (partial).
#[derive(Debug, serde::Deserialize)]
struct AccountInfo {
    /// Balance in SUN. May be absent for new/unfunded accounts.
    #[serde(default)]
    balance: u64,
}

/// Fetches TRON balances via the `TronGrid` REST API.
pub struct TronBalanceFetcher {
    endpoints: Endpoints,
}

impl TronBalanceFetcher {
    /// Create a balance fetcher from the configured API URLs.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            endpoints: Endpoints::new(api_urls.clone()),
        }
    }
}

#[async_trait]
impl BalanceFetcher for TronBalanceFetcher {
    /// Fetch the on-chain TRX balance for an address (in SUN).
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the API call fails, or
    /// `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn fetch_balance(&self, address: &Address, network: &NetworkId) -> Result<Amount> {
        let base = self.endpoints.primary(network)?;

        let account_url = base
            .join("/wallet/getaccount")
            .map_err(|e| DontYeetWalletError::Network(format!("URL join error: {e}")))?;

        let body = serde_json::json!({
            "address": address.as_str(),
            "visible": true,
        });

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let response = client
            .post_json(&account_url, &body)
            .await
            .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

        let info: AccountInfo = response
            .json()
            .map_err(|e| DontYeetWalletError::Network(format!("account info parse error: {e}")))?;

        Ok(Amount::from_raw(u128::from(info.balance), TRX_DECIMALS))
    }
}

// Rust guideline compliant 2026-05-02
