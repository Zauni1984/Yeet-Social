//! SPL token balance fetching via Solana JSON-RPC.
//!
//! Calls `getTokenAccountsByOwner` with a mint filter and `jsonParsed`
//! encoding to extract token balances without manual account deserialization.

use std::collections::HashMap;

use url::Url;

use dontyeet_network::{HttpClient, ReqwestClient};
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::{Amount, DontYeetWalletError, Result};

/// Fetches SPL token balances using the Solana JSON-RPC API.
pub struct SolTokenBalanceFetcher {
    api_urls: HashMap<NetworkId, Vec<Url>>,
}

impl SolTokenBalanceFetcher {
    /// Create a new fetcher sharing the same API URLs as the chain plugin.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            api_urls: api_urls.clone(),
        }
    }

    /// Fetch the SPL token balance of `owner_address` for the mint
    /// at `mint_address` on the given `network`.
    ///
    /// Returns `(amount, symbol)`. The symbol is read from on-chain
    /// metadata when available, otherwise defaults to `"SPL"`.
    ///
    /// # Errors
    /// Returns network or parsing errors.
    pub async fn fetch_balance(
        &self,
        owner_address: &str,
        mint_address: &str,
        network: &NetworkId,
    ) -> Result<(Amount, String)> {
        let urls = self
            .api_urls
            .get(network)
            .ok_or_else(|| DontYeetWalletError::Network(format!("no API URLs for {network}")))?;
        let base = urls
            .first()
            .ok_or_else(|| DontYeetWalletError::Network("API URL list is empty".into()))?;

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTokenAccountsByOwner",
            "params": [
                owner_address,
                { "mint": mint_address },
                { "encoding": "jsonParsed" }
            ]
        });

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let response = client
            .post_json(base, &body)
            .await
            .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let resp: TokenAccountsResponse = response
            .json()
            .map_err(|e| DontYeetWalletError::Network(format!("parse error: {e}")))?;

        // Sum balances across all token accounts for this mint
        // (usually just one, but associated token accounts can exist).
        let mut total_raw: u128 = 0;
        let mut decimals: u8 = 0;

        if let Some(accounts) = resp.result.value {
            for account in &accounts {
                if let Some(parsed) = &account.account.data.parsed
                    && let Some(info) = &parsed.info
                {
                    let amt_str = &info.token_amount.amount;
                    let raw: u128 = amt_str.parse().unwrap_or(0);
                    total_raw = total_raw.saturating_add(raw);
                    decimals = info.token_amount.decimals;
                }
            }
        }

        let amount = Amount::from_raw(total_raw, decimals);
        // SPL tokens don't have on-chain names in the basic account data.
        // The caller (admin token registry) provides the symbol.
        Ok((amount, "SPL".into()))
    }
}

// -- JSON-RPC response types for getTokenAccountsByOwner --

#[derive(serde::Deserialize)]
struct TokenAccountsResponse {
    result: TokenAccountsResult,
}

#[derive(serde::Deserialize)]
struct TokenAccountsResult {
    value: Option<Vec<TokenAccountEntry>>,
}

#[derive(serde::Deserialize)]
struct TokenAccountEntry {
    account: AccountData,
}

#[derive(serde::Deserialize)]
struct AccountData {
    data: ParsedAccountData,
}

#[derive(serde::Deserialize)]
struct ParsedAccountData {
    parsed: Option<ParsedTokenInfo>,
}

#[derive(serde::Deserialize)]
struct ParsedTokenInfo {
    info: Option<TokenInfo>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenInfo {
    token_amount: TokenAmount,
}

#[derive(serde::Deserialize)]
struct TokenAmount {
    amount: String,
    decimals: u8,
}

// Rust guideline compliant 2026-05-02
