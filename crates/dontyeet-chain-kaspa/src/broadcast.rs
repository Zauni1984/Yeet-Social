//! Kaspa transaction broadcasting.
//!
//! Submits signed transactions to the Kaspa network via the REST API
//! at `POST /transactions`.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::Deserialize;
use url::Url;

use dontyeet_network::Endpoints;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::TransactionBroadcaster;
use dontyeet_primitives::transaction::TxHash;

use crate::rest;

/// JSON response from `POST /transactions`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BroadcastResponse {
    /// The resulting transaction ID.
    transaction_id: String,
}

/// Broadcasts signed Kaspa transactions via the REST API.
pub struct KaspaBroadcaster {
    endpoints: Endpoints,
}

impl KaspaBroadcaster {
    /// Create a broadcaster from the configured API URLs.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            endpoints: Endpoints::new(api_urls.clone()),
        }
    }
}

#[async_trait]
impl TransactionBroadcaster for KaspaBroadcaster {
    /// Broadcast a signed transaction and return the resulting tx hash.
    ///
    /// The `signed_tx` bytes are expected to be a JSON-encoded Kaspa
    /// transaction object.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the API call fails, or
    /// `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn broadcast(&self, signed_tx: &[u8], network: &NetworkId) -> Result<TxHash> {
        let urls = self.endpoints.all(network)?;

        let tx_json: serde_json::Value = serde_json::from_slice(signed_tx)
            .map_err(|e| DontYeetWalletError::Chain(format!("invalid tx JSON: {e}")))?;

        let body: BroadcastResponse = rest::rest_post(urls, "transactions", &tx_json).await?;

        Ok(TxHash::new(body.transaction_id))
    }
}

// Rust guideline compliant 2026-05-02
