//! TRON transaction broadcasting via `TronGrid` REST API.
//!
//! Sends signed transactions to `POST /wallet/broadcasttransaction`
//! with the signed transaction JSON as the request body.

use std::collections::HashMap;

use async_trait::async_trait;
use url::Url;

use dontyeet_network::{Endpoints, HttpClient, ReqwestClient};
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::TransactionBroadcaster;
use dontyeet_primitives::transaction::TxHash;

/// `TronGrid` broadcast response (partial).
#[derive(Debug, serde::Deserialize)]
struct BroadcastResponse {
    /// Transaction ID returned by the node.
    #[serde(default)]
    txid: String,
    /// Whether the broadcast was successful.
    #[serde(default)]
    result: bool,
}

/// Broadcasts signed TRON transactions via `TronGrid`.
pub struct TronBroadcaster {
    endpoints: Endpoints,
}

impl TronBroadcaster {
    /// Create a broadcaster from the configured API URLs.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            endpoints: Endpoints::new(api_urls.clone()),
        }
    }
}

#[async_trait]
impl TransactionBroadcaster for TronBroadcaster {
    /// Broadcast a signed transaction and return the resulting txid.
    ///
    /// The `TronGrid` `POST /wallet/broadcasttransaction` endpoint expects
    /// the signed transaction JSON in the request body and returns a JSON
    /// object containing the `txid`.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the API call fails, or
    /// `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn broadcast(&self, signed_tx: &[u8], network: &NetworkId) -> Result<TxHash> {
        let base = self.endpoints.primary(network)?;

        let broadcast_url = base
            .join("/wallet/broadcasttransaction")
            .map_err(|e| DontYeetWalletError::Network(format!("URL join error: {e}")))?;

        // The signed transaction bytes are expected to be valid JSON.
        let body: serde_json::Value = serde_json::from_slice(signed_tx)
            .map_err(|e| DontYeetWalletError::Chain(format!("signed tx is not valid JSON: {e}")))?;

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let response = client
            .post_json(&broadcast_url, &body)
            .await
            .map_err(|e| DontYeetWalletError::Network(format!("broadcast error: {e}")))?;

        let broadcast_resp: BroadcastResponse = response
            .json()
            .map_err(|e| DontYeetWalletError::Network(format!("broadcast response parse: {e}")))?;

        if !broadcast_resp.result && broadcast_resp.txid.is_empty() {
            return Err(DontYeetWalletError::Chain(
                "TRON broadcast failed: no txid returned".into(),
            ));
        }

        Ok(TxHash::new(broadcast_resp.txid.trim()))
    }
}

// Rust guideline compliant 2026-05-02
