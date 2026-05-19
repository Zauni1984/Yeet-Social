//! Algorand transaction history via Indexer REST API.
//!
//! Fetches transactions for a given address from the Algorand Indexer
//! `/v2/accounts/{addr}/transactions` endpoint and maps them to
//! [`TxHistoryItem`] with direction, amount, and status.

use std::collections::HashMap;

use url::Url;

use dontyeet_network::{HttpClient, ReqwestClient, encode_segment};
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::transaction::{TxConfirmation, TxDirection, TxHash, TxHistoryItem};
use dontyeet_primitives::{Address, Amount, DontYeetWalletError, Result};

/// Fetches Algorand transaction history from the Indexer API.
pub struct AlgoHistoryFetcher {
    api_urls: HashMap<NetworkId, Vec<Url>>,
}

impl AlgoHistoryFetcher {
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
                "/v2/accounts/{address_seg}/transactions?limit={cap}"
            ))
            .map_err(|e| DontYeetWalletError::Network(format!("URL join error: {e}")))?;

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let response = client
            .get(&endpoint)
            .await
            .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

        let resp: IndexerResponse = serde_json::from_slice(&response.body)
            .map_err(|e| DontYeetWalletError::Network(format!("parse error: {e}")))?;

        let own = address.to_uppercase();
        let items = resp
            .transactions
            .unwrap_or_default()
            .iter()
            .map(|tx| map_tx(tx, &own))
            .collect();

        Ok(items)
    }
}

/// Map an Algorand Indexer transaction to a [`TxHistoryItem`].
fn map_tx(tx: &AlgoTx, own_address: &str) -> TxHistoryItem {
    let is_sender = tx.sender.to_uppercase() == *own_address;

    let direction = if is_sender {
        TxDirection::Out
    } else {
        TxDirection::In
    };

    let (amount_raw, counterparty) = if let Some(ref pay) = tx.payment_transaction {
        let cp = if is_sender {
            pay.receiver.clone().unwrap_or_default()
        } else {
            tx.sender.clone()
        };
        (u128::from(pay.amount.unwrap_or(0)), cp)
    } else {
        let cp = if is_sender {
            String::new()
        } else {
            tx.sender.clone()
        };
        (0, cp)
    };

    let status = if tx.confirmed_round.unwrap_or(0) > 0 {
        TxConfirmation::Confirmed
    } else {
        TxConfirmation::Pending
    };

    TxHistoryItem {
        tx_hash: TxHash::new(&tx.id),
        direction,
        counterparty: Address::new(&counterparty),
        amount: Amount::from_raw(amount_raw, 6),
        symbol: "ALGO".into(),
        timestamp: tx.round_time.map(u64::cast_signed),
        status,
    }
}

// -- Algorand Indexer JSON response types --

#[derive(serde::Deserialize)]
struct IndexerResponse {
    #[serde(default)]
    transactions: Option<Vec<AlgoTx>>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
struct AlgoTx {
    id: String,
    sender: String,
    #[serde(default)]
    confirmed_round: Option<u64>,
    #[serde(default)]
    round_time: Option<u64>,
    #[serde(default)]
    payment_transaction: Option<PaymentTx>,
}

#[derive(serde::Deserialize)]
struct PaymentTx {
    #[serde(default)]
    amount: Option<u64>,
    #[serde(default)]
    receiver: Option<String>,
}

// Rust guideline compliant 2026-05-02
