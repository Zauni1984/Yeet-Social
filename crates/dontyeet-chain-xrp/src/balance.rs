//! XRP Ledger balance fetching via JSON-RPC API.
//!
//! Queries the `account_info` method and extracts the `Balance` field
//! (denominated in drops, 1 XRP = 1,000,000 drops).

use std::collections::HashMap;

use async_trait::async_trait;
use url::Url;

use dontyeet_network::{Endpoints, HttpClient, ReqwestClient};
use dontyeet_primitives::address::Address;
use dontyeet_primitives::amount::Amount;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::BalanceFetcher;

/// XRP decimal places (1 XRP = 1,000,000 drops).
const XRP_DECIMALS: u8 = 6;

/// JSON-RPC response for `account_info`.
#[derive(Debug, serde::Deserialize)]
struct AccountInfoResponse {
    result: AccountInfoResult,
}

/// Inner result of the `account_info` response.
#[derive(Debug, serde::Deserialize)]
struct AccountInfoResult {
    account_data: Option<AccountData>,
    #[expect(
        dead_code,
        reason = "deserialized from XRPL wire response; kept to document the schema"
    )]
    status: Option<String>,
    error: Option<String>,
}

/// Account data from the XRP Ledger.
#[derive(Debug, serde::Deserialize)]
#[expect(
    non_snake_case,
    reason = "struct mirrors XRPL JSON-RPC schema where field names use PascalCase (Balance, etc.)"
)]
struct AccountData {
    Balance: String,
}

/// Fetches XRP balances via the XRP Ledger JSON-RPC API.
pub struct XrpBalanceFetcher {
    endpoints: Endpoints,
}

impl XrpBalanceFetcher {
    /// Create a balance fetcher from the configured API URLs.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            endpoints: Endpoints::new(api_urls.clone()),
        }
    }
}

#[async_trait]
impl BalanceFetcher for XrpBalanceFetcher {
    /// Fetch the on-chain XRP balance for an address (in drops).
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the API call fails, or
    /// `DontYeetWalletError::NotFound` if no API URLs exist for the network
    /// or the account is not found on the ledger.
    async fn fetch_balance(&self, address: &Address, network: &NetworkId) -> Result<Amount> {
        let base = self.endpoints.primary(network)?;

        let body = serde_json::json!({
            "method": "account_info",
            "params": [{
                "account": address.as_str(),
                "ledger_index": "validated"
            }]
        });

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let response = client
            .post_json(base, &body)
            .await
            .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

        let info: AccountInfoResponse = response
            .json()
            .map_err(|e| DontYeetWalletError::Network(format!("account_info parse error: {e}")))?;

        // Check for RPC-level errors (e.g. account not found).
        if let Some(err) = info.result.error {
            return Err(DontYeetWalletError::NotFound(format!(
                "XRP account_info error: {err}"
            )));
        }

        let account_data = info.result.account_data.ok_or_else(|| {
            DontYeetWalletError::NotFound("XRP account data missing from response".into())
        })?;

        let balance_drops: u64 = account_data
            .Balance
            .parse()
            .map_err(|e| DontYeetWalletError::Chain(format!("balance parse error: {e}")))?;

        Ok(Amount::from_raw(u128::from(balance_drops), XRP_DECIMALS))
    }
}

// Rust guideline compliant 2026-05-02
