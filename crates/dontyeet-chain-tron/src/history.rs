//! TRON transaction history via `TronGrid` REST API.
//!
//! Fetches transactions for a given address from the `TronGrid`
//! `/v1/accounts/{addr}/transactions` endpoint and maps them to
//! [`TxHistoryItem`] with direction, amount, and status.

use std::collections::HashMap;

use url::Url;

use dontyeet_network::{HttpClient, ReqwestClient, encode_segment};
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::transaction::{TxConfirmation, TxDirection, TxHash, TxHistoryItem};
use dontyeet_primitives::{Address, Amount, DontYeetWalletError, Result};

/// Fetches TRON transaction history from the `TronGrid` API.
pub struct TronHistoryFetcher {
    api_urls: HashMap<NetworkId, Vec<Url>>,
}

impl TronHistoryFetcher {
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
        let address_seg = encode_segment(address);
        let endpoint = base
            .join(&format!(
                "/v1/accounts/{address_seg}/transactions?limit={cap}&order_by=block_timestamp,desc"
            ))
            .map_err(|e| DontYeetWalletError::Network(format!("URL join error: {e}")))?;

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let response = client
            .get(&endpoint)
            .await
            .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

        let resp: TronGridResponse = serde_json::from_slice(&response.body)
            .map_err(|e| DontYeetWalletError::Network(format!("parse error: {e}")))?;

        let own = address.to_lowercase();
        let items = resp
            .data
            .unwrap_or_default()
            .iter()
            .map(|tx| map_tx(tx, &own))
            .collect();

        Ok(items)
    }
}

/// Map a `TronGrid` transaction to a [`TxHistoryItem`].
fn map_tx(tx: &TronTx, own_address: &str) -> TxHistoryItem {
    // Extract owner and destination from the first contract parameter.
    let (owner, to_addr, amount_raw) = tx
        .raw_data
        .as_ref()
        .and_then(|rd| rd.contract.as_ref())
        .and_then(|c| c.first())
        .and_then(|c| c.parameter.as_ref())
        .map(|p| {
            let o = p.value.owner_address.clone().unwrap_or_default();
            let t = p.value.to_address.clone().unwrap_or_default();
            let a = p.value.amount.unwrap_or(0);
            (o, t, a)
        })
        .unwrap_or_default();

    let is_sender = owner.to_lowercase() == *own_address;
    let direction = if is_sender {
        TxDirection::Out
    } else {
        TxDirection::In
    };

    let counterparty = if is_sender { &to_addr } else { &owner };

    // block_timestamp is in milliseconds; convert to seconds.
    let timestamp = tx.block_timestamp.map(|ms| ms / 1000);

    let status = if tx
        .ret
        .as_ref()
        .and_then(|r| r.first())
        .map(|r| r.contract_ret.as_deref())
        == Some(Some("SUCCESS"))
    {
        TxConfirmation::Confirmed
    } else {
        TxConfirmation::Pending
    };

    TxHistoryItem {
        tx_hash: TxHash::new(tx.tx_id.clone().unwrap_or_default()),
        direction,
        counterparty: Address::new(counterparty),
        amount: Amount::from_raw(u128::from(amount_raw), 6),
        symbol: "TRX".into(),
        timestamp,
        status,
    }
}

// -- TronGrid JSON response types --

#[derive(serde::Deserialize)]
struct TronGridResponse {
    #[serde(default)]
    data: Option<Vec<TronTx>>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TronTx {
    #[serde(default, rename = "txID")]
    tx_id: Option<String>,
    #[serde(default)]
    raw_data: Option<TronRawData>,
    #[serde(default)]
    block_timestamp: Option<i64>,
    #[serde(default)]
    ret: Option<Vec<TronRet>>,
}

#[derive(serde::Deserialize)]
struct TronRawData {
    #[serde(default)]
    contract: Option<Vec<TronContract>>,
}

#[derive(serde::Deserialize)]
struct TronContract {
    #[serde(default)]
    parameter: Option<TronParam>,
}

#[derive(serde::Deserialize)]
struct TronParam {
    value: TronParamValue,
}

#[derive(serde::Deserialize)]
struct TronParamValue {
    #[serde(default)]
    owner_address: Option<String>,
    #[serde(default)]
    to_address: Option<String>,
    #[serde(default)]
    amount: Option<u64>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TronRet {
    #[serde(default)]
    contract_ret: Option<String>,
}

// Rust guideline compliant 2026-05-02
