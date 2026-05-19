//! Kadena transaction broadcasting.
//!
//! Submits signed transactions to the Kadena network via the Pact API
//! at `POST /chainweb/0.0/{version}/chain/{chain}/pact/api/v1/send`.

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

/// Default chain to broadcast transactions on.
const DEFAULT_CHAIN: &str = "0";

/// JSON response from `POST .../pact/api/v1/send`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendResponse {
    /// The resulting request keys (transaction hashes).
    request_keys: Vec<String>,
}

/// Broadcasts signed Kadena transactions via the Pact API.
pub struct KadenaBroadcaster {
    endpoints: Endpoints,
    network_versions: HashMap<NetworkId, String>,
}

impl KadenaBroadcaster {
    /// Create a broadcaster from the configured API URLs.
    #[must_use]
    pub fn new(
        api_urls: &HashMap<NetworkId, Vec<Url>>,
        network_versions: &HashMap<NetworkId, String>,
    ) -> Self {
        Self {
            endpoints: Endpoints::new(api_urls.clone()),
            network_versions: network_versions.clone(),
        }
    }
}

#[async_trait]
impl TransactionBroadcaster for KadenaBroadcaster {
    /// Broadcast a signed transaction and return the resulting tx hash.
    ///
    /// The `signed_tx` bytes are expected to be a JSON-encoded Pact
    /// command object ready for `/send`.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the API call fails, or
    /// `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn broadcast(&self, signed_tx: &[u8], network: &NetworkId) -> Result<TxHash> {
        let urls = self.endpoints.all(network)?;

        let version = self
            .network_versions
            .get(network)
            .ok_or_else(|| DontYeetWalletError::NotFound(format!("no network version for {network}")))?;

        let path = format!("chainweb/0.0/{version}/chain/{DEFAULT_CHAIN}/pact/api/v1/send");

        let tx_json: serde_json::Value = serde_json::from_slice(signed_tx)
            .map_err(|e| DontYeetWalletError::Chain(format!("invalid tx JSON: {e}")))?;

        let body = serde_json::json!({
            "cmds": [tx_json]
        });

        let resp: SendResponse = rest::rest_post(urls, &path, &body).await?;

        let request_key = resp
            .request_keys
            .into_iter()
            .next()
            .ok_or_else(|| DontYeetWalletError::Chain("no request key in send response".into()))?;

        Ok(TxHash::new(request_key))
    }
}

// Rust guideline compliant 2026-05-02
