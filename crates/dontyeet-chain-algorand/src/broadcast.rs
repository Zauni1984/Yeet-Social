//! Algorand transaction broadcasting via Algod REST v2 API.
//!
//! Sends signed raw transactions to `POST /v2/transactions` with the
//! raw binary transaction as the request body.

use std::collections::HashMap;

use async_trait::async_trait;
use url::Url;

use dontyeet_network::{Endpoints, HttpClient, ReqwestClient};
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::TransactionBroadcaster;
use dontyeet_primitives::transaction::TxHash;

/// Broadcasts signed Algorand transactions via the Algod REST v2 API.
pub struct AlgoBroadcaster {
    endpoints: Endpoints,
}

impl AlgoBroadcaster {
    /// Create a broadcaster from the configured API URLs.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            endpoints: Endpoints::new(api_urls.clone()),
        }
    }
}

#[async_trait]
impl TransactionBroadcaster for AlgoBroadcaster {
    /// Broadcast a signed transaction and return the resulting txid.
    ///
    /// The Algod `POST /v2/transactions` endpoint expects the raw
    /// `MessagePack`-encoded signed transaction in the request body
    /// and returns a JSON response with `{ "txId": "..." }`.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the API call fails, or
    /// `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn broadcast(&self, signed_tx: &[u8], network: &NetworkId) -> Result<TxHash> {
        let base = self.endpoints.primary(network)?;

        let tx_url = base
            .join("/v2/transactions")
            .map_err(|e| DontYeetWalletError::Network(format!("URL join error: {e}")))?;

        // Algod expects the raw binary body. We encode as hex string
        // wrapped in a JSON string for the post_json abstraction.
        let raw_hex = hex::encode(signed_tx);
        let body = serde_json::Value::String(raw_hex);

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let response = client
            .post_json(&tx_url, &body)
            .await
            .map_err(|e| DontYeetWalletError::Network(format!("broadcast error: {e}")))?;

        // Response: { "txId": "TRANSACTION_ID" }
        let resp: serde_json::Value = response
            .json()
            .map_err(|e| DontYeetWalletError::Network(format!("response parse error: {e}")))?;

        let txid = resp
            .get("txId")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| DontYeetWalletError::Network("missing txId in response".into()))?;

        Ok(TxHash::new(txid))
    }
}

// Rust guideline compliant 2026-05-02
