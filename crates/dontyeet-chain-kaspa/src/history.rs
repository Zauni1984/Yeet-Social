//! Kaspa transaction history via REST API.
//!
//! Fetches full transactions for a given address from the Kaspa REST
//! endpoint and maps them to [`TxHistoryItem`] with direction, amount,
//! and status.

use std::collections::HashMap;

use url::Url;

use dontyeet_network::{HttpClient, ReqwestClient, encode_segment};
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::transaction::{TxConfirmation, TxDirection, TxHash, TxHistoryItem};
use dontyeet_primitives::{Address, Amount, DontYeetWalletError, Result};

/// Fetches Kaspa transaction history from the REST API.
pub struct KaspaHistoryFetcher {
    api_urls: HashMap<NetworkId, Vec<Url>>,
}

impl KaspaHistoryFetcher {
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
                "/addresses/{address_seg}/full-transactions?limit={cap}&resolve_previous_outpoints=light"
            ))
            .map_err(|e| DontYeetWalletError::Network(format!("URL join error: {e}")))?;

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let response = client
            .get(&endpoint)
            .await
            .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

        let txs: Vec<KaspaTx> = serde_json::from_slice(&response.body)
            .map_err(|e| DontYeetWalletError::Network(format!("parse error: {e}")))?;

        let own = address.to_lowercase();
        let items = txs.iter().map(|tx| map_tx(tx, &own)).collect();
        Ok(items)
    }
}

/// Map a Kaspa REST transaction to a [`TxHistoryItem`].
fn map_tx(tx: &KaspaTx, own_address: &str) -> TxHistoryItem {
    // If any input address matches own address, this is an outgoing tx.
    let is_sender = tx.inputs.as_ref().is_some_and(|inputs| {
        inputs.iter().any(|i| {
            i.previous_outpoint_address
                .as_deref()
                .is_some_and(|a| a.to_lowercase() == *own_address)
        })
    });

    let direction = if is_sender {
        TxDirection::Out
    } else {
        TxDirection::In
    };

    // For outgoing: sum outputs NOT to own address. For incoming: sum outputs TO own address.
    let outputs = tx.outputs.as_deref().unwrap_or_default();
    let amount_sompi: u64 = outputs
        .iter()
        .filter(|o| {
            let addr_match = o
                .script_public_key_address
                .as_deref()
                .is_some_and(|a| a.to_lowercase() == *own_address);
            if is_sender { !addr_match } else { addr_match }
        })
        .map(|o| o.amount.unwrap_or(0))
        .sum();

    // Counterparty: first non-own output for outgoing, first input for incoming.
    let counterparty = if is_sender {
        outputs
            .iter()
            .find(|o| {
                o.script_public_key_address
                    .as_deref()
                    .is_some_and(|a| a.to_lowercase() != *own_address)
            })
            .and_then(|o| o.script_public_key_address.clone())
            .unwrap_or_default()
    } else {
        tx.inputs
            .as_ref()
            .and_then(|inputs| inputs.first())
            .and_then(|i| i.previous_outpoint_address.clone())
            .unwrap_or_default()
    };

    // block_time is in milliseconds; convert to seconds.
    let timestamp = tx.block_time.map(|ms| ms / 1000);

    TxHistoryItem {
        tx_hash: TxHash::new(tx.transaction_id.clone().unwrap_or_default()),
        direction,
        counterparty: Address::new(&counterparty),
        amount: Amount::from_raw(u128::from(amount_sompi), 8),
        symbol: "KAS".into(),
        timestamp,
        status: TxConfirmation::Confirmed,
    }
}

// -- Kaspa REST JSON response types --

#[derive(serde::Deserialize)]
struct KaspaTx {
    #[serde(default)]
    transaction_id: Option<String>,
    #[serde(default)]
    inputs: Option<Vec<KaspaInput>>,
    #[serde(default)]
    outputs: Option<Vec<KaspaOutput>>,
    #[serde(default)]
    block_time: Option<i64>,
}

#[derive(serde::Deserialize)]
struct KaspaInput {
    #[serde(default)]
    previous_outpoint_address: Option<String>,
}

#[derive(serde::Deserialize)]
struct KaspaOutput {
    #[serde(default)]
    script_public_key_address: Option<String>,
    #[serde(default)]
    amount: Option<u64>,
}

// Rust guideline compliant 2026-05-02
