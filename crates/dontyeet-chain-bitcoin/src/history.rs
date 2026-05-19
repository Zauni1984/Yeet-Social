//! Bitcoin transaction history via Mempool.space REST API.
//!
//! Fetches transactions for a given address and maps them to
//! [`TxHistoryItem`] with direction/amount/status.

use std::collections::HashMap;

use url::Url;

use dontyeet_network::{HttpClient, ReqwestClient, encode_segment};
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::transaction::{TxConfirmation, TxDirection, TxHash, TxHistoryItem};
use dontyeet_primitives::{Address, Amount, DontYeetWalletError, Result};

/// Fetches Bitcoin transaction history from Mempool.space.
pub struct BtcHistoryFetcher {
    api_urls: HashMap<NetworkId, Vec<Url>>,
}

impl BtcHistoryFetcher {
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

        let address_seg = encode_segment(address);
        let endpoint = base
            .join(&format!("/address/{address_seg}/txs"))
            .map_err(|e| DontYeetWalletError::Network(format!("URL join error: {e}")))?;

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let response = client
            .get(&endpoint)
            .await
            .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

        let txs: Vec<MempoolTx> = serde_json::from_slice(&response.body)
            .map_err(|e| DontYeetWalletError::Network(format!("parse error: {e}")))?;

        let own = address.to_lowercase();
        let capped = if limit == 0 {
            txs.len()
        } else {
            txs.len().min(limit)
        };

        let items = txs[..capped].iter().map(|tx| map_tx(tx, &own)).collect();

        Ok(items)
    }
}

/// Map a Mempool.space transaction to a [`TxHistoryItem`].
fn map_tx(tx: &MempoolTx, own_address: &str) -> TxHistoryItem {
    // Direction: if any input is from own address, it's outgoing.
    let is_sender = tx.vin.iter().any(|vin| {
        vin.prevout
            .as_ref()
            .and_then(|p| p.scriptpubkey_address.as_deref())
            .is_some_and(|a| a.to_lowercase() == *own_address)
    });

    let direction = if is_sender {
        TxDirection::Out
    } else {
        TxDirection::In
    };

    // Amount + counterparty
    let (amount_sats, counterparty) = if is_sender {
        // Outgoing: total sent to non-own addresses.
        let sent: u64 = tx
            .vout
            .iter()
            .filter(|v| {
                v.scriptpubkey_address
                    .as_deref()
                    .is_some_and(|a| a.to_lowercase() != *own_address)
            })
            .map(|v| v.value)
            .sum();
        let recipient = tx
            .vout
            .iter()
            .find(|v| {
                v.scriptpubkey_address
                    .as_deref()
                    .is_some_and(|a| a.to_lowercase() != *own_address)
            })
            .and_then(|v| v.scriptpubkey_address.clone())
            .unwrap_or_default();
        (sent, recipient)
    } else {
        // Incoming: total received at own address.
        let received: u64 = tx
            .vout
            .iter()
            .filter(|v| {
                v.scriptpubkey_address
                    .as_deref()
                    .is_some_and(|a| a.to_lowercase() == *own_address)
            })
            .map(|v| v.value)
            .sum();
        let sender = tx
            .vin
            .first()
            .and_then(|vin| vin.prevout.as_ref())
            .and_then(|p| p.scriptpubkey_address.clone())
            .unwrap_or_default();
        (received, sender)
    };

    let status = if tx.status.confirmed {
        TxConfirmation::Confirmed
    } else {
        TxConfirmation::Pending
    };

    TxHistoryItem {
        tx_hash: TxHash::new(&tx.txid),
        direction,
        counterparty: Address::new(&counterparty),
        amount: Amount::from_raw(u128::from(amount_sats), 8),
        symbol: "BTC".into(),
        timestamp: tx.status.block_time,
        status,
    }
}

// -- Mempool.space JSON response types --

#[derive(serde::Deserialize)]
struct MempoolTx {
    txid: String,
    vin: Vec<Vin>,
    vout: Vec<Vout>,
    status: TxStatus,
}

#[derive(serde::Deserialize)]
struct Vin {
    prevout: Option<Prevout>,
}

#[derive(serde::Deserialize)]
struct Prevout {
    scriptpubkey_address: Option<String>,
}

#[derive(serde::Deserialize)]
struct Vout {
    scriptpubkey_address: Option<String>,
    value: u64,
}

#[derive(serde::Deserialize)]
struct TxStatus {
    confirmed: bool,
    block_time: Option<i64>,
}

// Rust guideline compliant 2026-05-02
