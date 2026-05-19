//! Cardano transaction history via Blockfrost REST API.
//!
//! Fetches the transaction list for a given address from the Blockfrost
//! `/addresses/{addr}/transactions` endpoint and maps entries to
//! [`TxHistoryItem`].
//!
//! Detailed amount/direction data would require a second call per tx
//! (`/txs/{hash}/utxos`), so we return confirmed hashes with timestamps.

use std::collections::HashMap;

use url::Url;

use dontyeet_network::{HttpClient, ReqwestClient, encode_segment};
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::transaction::{TxConfirmation, TxDirection, TxHash, TxHistoryItem};
use dontyeet_primitives::{Address, Amount, DontYeetWalletError, Result};

/// Fetches Cardano transaction history from the Blockfrost API.
pub struct CardanoHistoryFetcher {
    api_urls: HashMap<NetworkId, Vec<Url>>,
    project_id: Option<String>,
}

impl CardanoHistoryFetcher {
    /// Create a new fetcher sharing the same API URLs and `project_id` as
    /// the chain plugin.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>, project_id: Option<String>) -> Self {
        Self {
            api_urls: api_urls.clone(),
            project_id,
        }
    }

    /// Fetch transaction history for `address` on `network`.
    ///
    /// Returns transaction hashes with block timestamps. Amount and
    /// direction are unavailable without per-tx UTXO lookups.
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
                "/addresses/{address_seg}/transactions?order=desc&count={cap}"
            ))
            .map_err(|e| DontYeetWalletError::Network(format!("URL join error: {e}")))?;

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let headers = crate::auth::project_id_headers(self.project_id.as_deref());
        let response = client
            .get_with_headers(&endpoint, &headers)
            .await
            .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

        let txs: Vec<BlockfrostTx> = serde_json::from_slice(&response.body)
            .map_err(|e| DontYeetWalletError::Network(format!("parse error: {e}")))?;

        let items = txs.iter().map(map_tx).collect();
        Ok(items)
    }
}

/// Map a Blockfrost transaction entry to a [`TxHistoryItem`].
fn map_tx(tx: &BlockfrostTx) -> TxHistoryItem {
    TxHistoryItem {
        tx_hash: TxHash::new(&tx.tx_hash),
        direction: TxDirection::Out,
        counterparty: Address::new(""),
        amount: Amount::from_raw(0, 6),
        symbol: "ADA".into(),
        timestamp: Some(tx.block_time),
        status: TxConfirmation::Confirmed,
    }
}

// -- Blockfrost JSON response types --

#[derive(serde::Deserialize)]
struct BlockfrostTx {
    tx_hash: String,
    #[expect(
        dead_code,
        reason = "deserialized from Blockfrost wire response; kept for future use and to document the schema"
    )]
    tx_index: u64,
    #[expect(
        dead_code,
        reason = "deserialized from Blockfrost wire response; kept for future use and to document the schema"
    )]
    block_height: u64,
    block_time: i64,
}

// Rust guideline compliant 2026-05-02
