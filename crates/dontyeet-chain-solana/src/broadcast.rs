//! Solana transaction broadcasting via JSON-RPC API.
//!
//! Sends signed transactions using `sendTransaction` with the
//! Base64-encoded signed transaction as the parameter.

use std::collections::HashMap;

use async_trait::async_trait;
use data_encoding::BASE64;
use url::Url;

use dontyeet_network::{Endpoints, HttpClient, ReqwestClient};
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::TransactionBroadcaster;
use dontyeet_primitives::transaction::TxHash;

/// JSON-RPC response for `sendTransaction`.
#[derive(Debug, serde::Deserialize)]
struct SendTxResponse {
    result: String,
}

/// Broadcasts signed Solana transactions via JSON-RPC.
pub struct SolBroadcaster {
    endpoints: Endpoints,
}

impl SolBroadcaster {
    /// Create a broadcaster from the configured API URLs.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            endpoints: Endpoints::new(api_urls.clone()),
        }
    }
}

#[async_trait]
impl TransactionBroadcaster for SolBroadcaster {
    /// Broadcast a signed transaction and return the resulting tx signature.
    ///
    /// The Solana `sendTransaction` RPC method expects a Base64-encoded
    /// signed transaction and returns the transaction signature hash.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the API call fails, or
    /// `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn broadcast(&self, signed_tx: &[u8], network: &NetworkId) -> Result<TxHash> {
        let base = self.endpoints.primary(network)?;

        let encoded_tx = BASE64.encode(signed_tx);

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sendTransaction",
            "params": [encoded_tx]
        });

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let response = client
            .post_json(base, &body)
            .await
            .map_err(|e| DontYeetWalletError::Network(format!("broadcast error: {e}")))?;

        let tx_resp: SendTxResponse = response
            .json()
            .map_err(|e| DontYeetWalletError::Network(format!("tx response parse error: {e}")))?;

        Ok(TxHash::new(tx_resp.result))
    }
}

// Rust guideline compliant 2026-05-02
