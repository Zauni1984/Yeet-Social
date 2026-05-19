//! EVM transaction history via Etherscan-compatible APIs.
//!
//! Fetches transaction lists from block explorer APIs (Etherscan,
//! Polygonscan, `BscScan`, Snowtrace) and maps them to
//! [`TxHistoryItem`].

use std::collections::HashMap;
use std::fmt::Write;

use url::Url;

use dontyeet_network::{HttpClient, ReqwestClient};
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::transaction::{TxConfirmation, TxDirection, TxHash, TxHistoryItem};
use dontyeet_primitives::{Address, Amount, DontYeetWalletError, Result};

/// Fetches EVM transaction history from Etherscan-compatible APIs.
pub struct EvmHistoryFetcher {
    explorer_api_urls: HashMap<NetworkId, Url>,
    api_key: Option<String>,
    symbol: String,
}

impl EvmHistoryFetcher {
    /// Create a new fetcher.
    #[must_use]
    pub fn new(
        explorer_api_urls: &HashMap<NetworkId, Url>,
        api_key: &Option<String>,
        symbol: &str,
    ) -> Self {
        Self {
            explorer_api_urls: explorer_api_urls.clone(),
            api_key: api_key.clone(),
            symbol: symbol.to_string(),
        }
    }

    /// Fetch transaction history for `address` on `network`.
    ///
    /// # Errors
    /// Returns `Unsupported` if no explorer API is configured for this
    /// network, or network/parsing errors.
    pub async fn fetch(
        &self,
        address: &str,
        network: &NetworkId,
        limit: usize,
    ) -> Result<Vec<TxHistoryItem>> {
        let base = self
            .explorer_api_urls
            .get(network)
            .ok_or_else(|| DontYeetWalletError::Unsupported(format!("no explorer API for {network}")))?;

        let offset = if limit == 0 { 25 } else { limit };
        let mut endpoint = format!(
            "{base}?module=account&action=txlist&address={address}&sort=desc&page=1&offset={offset}"
        );
        if let Some(key) = &self.api_key {
            let _ = write!(endpoint, "&apikey={key}");
        }

        let url: Url = endpoint
            .parse()
            .map_err(|e| DontYeetWalletError::Network(format!("URL parse error: {e}")))?;

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let response = client
            .get(&url)
            .await
            .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

        let resp: EtherscanResponse = serde_json::from_slice(&response.body)
            .map_err(|e| DontYeetWalletError::Network(format!("parse error: {e}")))?;

        let own = address.to_lowercase();
        let items = resp.result.iter().map(|tx| self.map_tx(tx, &own)).collect();

        Ok(items)
    }

    /// Map an Etherscan transaction to a [`TxHistoryItem`].
    fn map_tx(&self, tx: &EtherscanTx, own_address: &str) -> TxHistoryItem {
        let from_lower = tx.from.to_lowercase();

        let is_sender = from_lower == *own_address;
        let direction = if is_sender {
            TxDirection::Out
        } else {
            TxDirection::In
        };

        let counterparty = if is_sender {
            tx.to.clone()
        } else {
            tx.from.clone()
        };

        // Value is in wei (decimal string).
        let status = if tx.is_error == "1" {
            TxConfirmation::Failed
        } else if tx.confirmations.parse::<u64>().unwrap_or(0) > 0 {
            TxConfirmation::Confirmed
        } else {
            TxConfirmation::Pending
        };

        let timestamp = tx.time_stamp.parse::<i64>().ok();

        // Parse wei value to Amount (18 decimals for EVM).
        let raw: u128 = tx.value.parse().unwrap_or(0);

        TxHistoryItem {
            tx_hash: TxHash::new(&tx.hash),
            direction,
            counterparty: Address::new(&counterparty),
            amount: Amount::from_raw(raw, 18),
            symbol: self.symbol.clone(),
            timestamp,
            status,
        }
    }
}

// -- Etherscan JSON response types --

#[derive(serde::Deserialize)]
struct EtherscanResponse {
    #[serde(default)]
    result: Vec<EtherscanTx>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct EtherscanTx {
    hash: String,
    from: String,
    to: String,
    value: String,
    time_stamp: String,
    #[serde(default)]
    is_error: String,
    #[serde(default)]
    confirmations: String,
}

// Rust guideline compliant 2026-05-02
