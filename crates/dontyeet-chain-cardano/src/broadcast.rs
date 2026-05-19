//! Cardano transaction broadcasting via Blockfrost REST API.
//!
//! Sends signed CBOR-encoded transactions to `POST /tx/submit`.

use std::collections::HashMap;

use async_trait::async_trait;
use url::Url;

use dontyeet_network::{Endpoints, HttpClient, ReqwestClient};
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::TransactionBroadcaster;
use dontyeet_primitives::transaction::TxHash;

/// Broadcasts signed Cardano transactions via Blockfrost.
pub struct CardanoBroadcaster {
    endpoints: Endpoints,
    project_id: Option<String>,
}

impl CardanoBroadcaster {
    /// Create a broadcaster from the configured API URLs and `project_id`.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>, project_id: Option<String>) -> Self {
        Self {
            endpoints: Endpoints::new(api_urls.clone()),
            project_id,
        }
    }
}

#[async_trait]
impl TransactionBroadcaster for CardanoBroadcaster {
    /// Broadcast a signed CBOR-encoded transaction.
    ///
    /// Blockfrost `POST /tx/submit` expects the raw CBOR bytes.
    /// Returns the transaction hash as a hex string.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the API call fails, or
    /// `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn broadcast(&self, signed_tx: &[u8], network: &NetworkId) -> Result<TxHash> {
        let base = self.endpoints.primary(network)?;

        let tx_url = base
            .join("/tx/submit")
            .map_err(|e| DontYeetWalletError::Network(format!("URL join error: {e}")))?;

        // Blockfrost expects CBOR body. We hex-encode and send as
        // a JSON string through our abstraction.
        let raw_hex = hex::encode(signed_tx);
        let body = serde_json::Value::String(raw_hex);

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let headers = crate::auth::project_id_headers(self.project_id.as_deref());
        let response = client
            .post_json_with_headers(&tx_url, &body, &headers)
            .await
            .map_err(|e| DontYeetWalletError::Network(format!("broadcast error: {e}")))?;

        // Blockfrost returns the tx hash as a plain JSON string.
        let txid = String::from_utf8(response.body)
            .map_err(|e| DontYeetWalletError::Network(format!("txid decode error: {e}")))?;

        Ok(TxHash::new(txid.trim().trim_matches('"')))
    }
}

// Rust guideline compliant 2026-05-02
