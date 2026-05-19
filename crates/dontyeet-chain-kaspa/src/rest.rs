//! Internal REST API helper for Kaspa.
//!
//! Provides thin wrappers that create a temporary [`ReqwestClient`] per
//! call using the first available URL from a list.

use serde::de::DeserializeOwned;
use serde_json::Value;
use url::Url;

use dontyeet_network::{HttpClient, ReqwestClient};
use dontyeet_primitives::error::{DontYeetWalletError, Result};

/// Send a GET request to `{base_url}/{path}` and deserialize the JSON
/// response.
///
/// # Errors
/// Returns `DontYeetWalletError::Network` if the request fails or the
/// response cannot be parsed.
pub async fn rest_get<T: DeserializeOwned>(urls: &[Url], path: &str) -> Result<T> {
    if urls.is_empty() {
        return Err(DontYeetWalletError::Network(
            "no API URLs configured for this Kaspa network".into(),
        ));
    }

    let base = urls[0].as_str().trim_end_matches('/');
    let full_url: Url = format!("{base}/{path}")
        .parse()
        .map_err(|e| DontYeetWalletError::Network(format!("invalid URL: {e}")))?;

    let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

    let resp = client
        .get(&full_url)
        .await
        .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

    resp.json()
        .map_err(|e| DontYeetWalletError::Network(e.to_string()))
}

/// Send a POST request with a JSON body to `{base_url}/{path}` and
/// deserialize the JSON response.
///
/// # Errors
/// Returns `DontYeetWalletError::Network` if the request fails or the
/// response cannot be parsed.
pub async fn rest_post<T: DeserializeOwned>(urls: &[Url], path: &str, body: &Value) -> Result<T> {
    if urls.is_empty() {
        return Err(DontYeetWalletError::Network(
            "no API URLs configured for this Kaspa network".into(),
        ));
    }

    let base = urls[0].as_str().trim_end_matches('/');
    let full_url: Url = format!("{base}/{path}")
        .parse()
        .map_err(|e| DontYeetWalletError::Network(format!("invalid URL: {e}")))?;

    let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

    let resp = client
        .post_json(&full_url, body)
        .await
        .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

    resp.json()
        .map_err(|e| DontYeetWalletError::Network(e.to_string()))
}

// Rust guideline compliant 2026-05-02
