//! XRP Ledger transaction broadcasting via JSON-RPC API.
//!
//! Sends signed transactions to the `submit` method with the
//! hex-encoded transaction blob.

use std::collections::HashMap;

use async_trait::async_trait;
use url::Url;

use dontyeet_network::{Endpoints, HttpClient, ReqwestClient};
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::TransactionBroadcaster;
use dontyeet_primitives::transaction::TxHash;

/// JSON-RPC response for the `submit` method.
#[derive(Debug, serde::Deserialize)]
struct SubmitResponse {
    result: SubmitResult,
}

/// Inner result of the `submit` response.
#[derive(Debug, serde::Deserialize)]
struct SubmitResult {
    tx_json: Option<SubmitTxJson>,
    #[expect(
        dead_code,
        reason = "deserialized from XRPL submit response; kept to document the schema"
    )]
    engine_result: Option<String>,
    error: Option<String>,
}

/// Transaction JSON returned from `submit`.
#[derive(Debug, serde::Deserialize)]
struct SubmitTxJson {
    hash: String,
}

/// Broadcasts signed XRP transactions via the JSON-RPC API.
pub struct XrpBroadcaster {
    endpoints: Endpoints,
}

impl XrpBroadcaster {
    /// Create a broadcaster from the configured API URLs.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            endpoints: Endpoints::new(api_urls.clone()),
        }
    }
}

#[async_trait]
impl TransactionBroadcaster for XrpBroadcaster {
    /// Broadcast a signed transaction and return the resulting tx hash.
    ///
    /// The `submit` method expects a hex-encoded transaction blob in
    /// the `tx_blob` parameter.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the API call fails, or
    /// `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn broadcast(&self, signed_tx: &[u8], network: &NetworkId) -> Result<TxHash> {
        let base = self.endpoints.primary(network)?;

        let tx_blob = hex::encode(signed_tx).to_uppercase();

        let body = serde_json::json!({
            "method": "submit",
            "params": [{
                "tx_blob": tx_blob
            }]
        });

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let response = client
            .post_json(base, &body)
            .await
            .map_err(|e| DontYeetWalletError::Network(format!("broadcast error: {e}")))?;

        let submit: SubmitResponse = response
            .json()
            .map_err(|e| DontYeetWalletError::Network(format!("submit parse error: {e}")))?;

        // Check for RPC-level errors.
        if let Some(err) = submit.result.error {
            return Err(DontYeetWalletError::Network(format!("XRP submit error: {err}")));
        }

        let tx_json = submit
            .result
            .tx_json
            .ok_or_else(|| DontYeetWalletError::Network("XRP submit response missing tx_json".into()))?;

        Ok(TxHash::new(tx_json.hash))
    }
}

// Rust guideline compliant 2026-05-02
