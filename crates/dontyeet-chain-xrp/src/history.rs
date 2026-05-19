//! XRP Ledger transaction history via JSON-RPC `account_tx`.
//!
//! Fetches transactions for a given address and maps them to
//! [`TxHistoryItem`] with direction, amount, and status.

use std::collections::HashMap;

use url::Url;

use dontyeet_network::{HttpClient, ReqwestClient};
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::transaction::{TxConfirmation, TxDirection, TxHash, TxHistoryItem};
use dontyeet_primitives::{Address, Amount, DontYeetWalletError, Result};

/// Ripple epoch offset: seconds between Unix epoch and 2000-01-01T00:00:00Z.
const RIPPLE_EPOCH_OFFSET: i64 = 946_684_800;

/// Fetches XRP Ledger transaction history from a JSON-RPC endpoint.
pub struct XrpHistoryFetcher {
    api_urls: HashMap<NetworkId, Vec<Url>>,
}

impl XrpHistoryFetcher {
    /// Create a new fetcher sharing the same API URLs as the chain plugin.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            api_urls: api_urls.clone(),
        }
    }

    /// Fetch transaction history for `address` on `network`.
    ///
    /// # Errors
    /// Returns network or parsing errors.
    pub async fn fetch(
        &self,
        address: &str,
        network: &NetworkId,
        limit: usize,
    ) -> Result<Vec<TxHistoryItem>> {
        let urls = self
            .api_urls
            .get(network)
            .ok_or_else(|| DontYeetWalletError::Network(format!("no API URLs for {network}")))?;
        let base = urls
            .first()
            .ok_or_else(|| DontYeetWalletError::Network("API URL list is empty".into()))?;

        let cap = if limit == 0 { 25 } else { limit };
        let body = serde_json::json!({
            "method": "account_tx",
            "params": [{"account": address, "limit": cap}]
        });

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let response = client
            .post_json(base, &body)
            .await
            .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

        let resp: RpcResponse = serde_json::from_slice(&response.body)
            .map_err(|e| DontYeetWalletError::Network(format!("parse error: {e}")))?;

        let own = address.to_lowercase();
        let items = resp
            .result
            .transactions
            .unwrap_or_default()
            .iter()
            .map(|entry| map_tx(entry, &own))
            .collect();

        Ok(items)
    }
}

/// Map an XRP transaction entry to a [`TxHistoryItem`].
fn map_tx(entry: &TxEntry, own_address: &str) -> TxHistoryItem {
    let tx = &entry.tx;
    let is_sender = tx.account.to_lowercase() == *own_address;

    let direction = if is_sender {
        TxDirection::Out
    } else {
        TxDirection::In
    };

    let counterparty = if is_sender {
        tx.destination.clone().unwrap_or_default()
    } else {
        tx.account.clone()
    };

    // Amount in drops (1 XRP = 1,000,000 drops). Can be a string or object
    // for non-XRP payments; we only parse string (native XRP).
    let drops: u128 = entry
        .meta
        .as_ref()
        .and_then(|m| m.delivered_amount.as_ref())
        .or(tx.amount.as_ref())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let status = if entry.validated.unwrap_or(false) {
        TxConfirmation::Confirmed
    } else {
        TxConfirmation::Pending
    };

    // Ripple epoch -> Unix timestamp.
    let timestamp = tx.date.map(|d| d + RIPPLE_EPOCH_OFFSET);

    TxHistoryItem {
        tx_hash: TxHash::new(tx.hash.clone().unwrap_or_default()),
        direction,
        counterparty: Address::new(&counterparty),
        amount: Amount::from_raw(drops, 6),
        symbol: "XRP".into(),
        timestamp,
        status,
    }
}

// -- XRP JSON-RPC response types --

#[derive(serde::Deserialize)]
struct RpcResponse {
    result: AccountTxResult,
}

#[derive(serde::Deserialize)]
struct AccountTxResult {
    #[serde(default)]
    transactions: Option<Vec<TxEntry>>,
}

#[derive(serde::Deserialize)]
struct TxEntry {
    tx: XrpTx,
    #[serde(default)]
    meta: Option<TxMeta>,
    #[serde(default)]
    validated: Option<bool>,
}

#[derive(serde::Deserialize)]
struct XrpTx {
    #[serde(default)]
    hash: Option<String>,
    #[serde(default, rename = "Account")]
    account: String,
    #[serde(default, rename = "Destination")]
    destination: Option<String>,
    #[serde(default, rename = "Amount")]
    amount: Option<String>,
    #[serde(default)]
    date: Option<i64>,
}

#[derive(serde::Deserialize)]
struct TxMeta {
    #[serde(default)]
    delivered_amount: Option<String>,
}

// Rust guideline compliant 2026-05-02
