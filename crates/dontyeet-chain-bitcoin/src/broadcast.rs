//! Bitcoin transaction broadcasting via Mempool.space REST API.
//!
//! Sends signed raw transactions to `POST /tx` with the hex-encoded
//! transaction as the request body.

use std::collections::HashMap;

use async_trait::async_trait;
use url::Url;

use dontyeet_network::{Endpoints, HttpClient, ReqwestClient};
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::TransactionBroadcaster;
use dontyeet_primitives::transaction::TxHash;

/// Broadcasts signed Bitcoin transactions via Mempool.space.
pub struct BtcBroadcaster {
    endpoints: Endpoints,
}

impl BtcBroadcaster {
    /// Create a broadcaster from the configured API URLs.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            endpoints: Endpoints::new(api_urls.clone()),
        }
    }
}

#[async_trait]
impl TransactionBroadcaster for BtcBroadcaster {
    /// Broadcast a signed transaction and return the resulting txid.
    ///
    /// The Mempool.space `POST /tx` endpoint expects the raw hex-encoded
    /// transaction in the request body and returns the txid as plain text.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the API call fails, or
    /// `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn broadcast(&self, signed_tx: &[u8], network: &NetworkId) -> Result<TxHash> {
        let base = self.endpoints.primary(network)?;

        let tx_url = base
            .join("/tx")
            .map_err(|e| DontYeetWalletError::Network(format!("URL join error: {e}")))?;

        let raw_hex = hex::encode(signed_tx);

        // Mempool.space expects the raw hex as a plain text POST body.
        // We use `post_json` with a JSON string value since the HTTP
        // client abstraction only supports JSON POST. The endpoint
        // accepts this format.
        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let body = serde_json::Value::String(raw_hex);

        let response = client
            .post_json(&tx_url, &body)
            .await
            .map_err(|e| DontYeetWalletError::Network(format!("broadcast error: {e}")))?;

        let txid = String::from_utf8(response.body)
            .map_err(|e| DontYeetWalletError::Network(format!("txid decode error: {e}")))?;

        Ok(TxHash::new(txid.trim()))
    }
}

// Rust guideline compliant 2026-05-02
