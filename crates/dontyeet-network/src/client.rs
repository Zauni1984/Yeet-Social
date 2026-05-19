//! HTTP client trait and `reqwest`-backed implementation.

use async_trait::async_trait;
use serde_json::Value;
use std::time::Duration;
use url::Url;

use crate::error::{NetworkError, NetworkResult};
use crate::privacy::PrivacyMode;

/// A raw HTTP response.
#[derive(Debug, Clone)]
pub struct Response {
    /// HTTP status code.
    pub status: u16,
    /// Response body bytes.
    pub body: Vec<u8>,
}

impl Response {
    /// Parse body as JSON.
    ///
    /// # Errors
    /// Returns `NetworkError::Deserialize` if the body is not valid JSON.
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> NetworkResult<T> {
        serde_json::from_slice(&self.body).map_err(|e| NetworkError::Deserialize(format!("{e}")))
    }
}

/// HTTP client abstraction.
///
/// Other crates depend on this trait, not on `reqwest` directly.
#[async_trait]
pub trait HttpClient: Send + Sync {
    /// Send a GET request.
    ///
    /// # Errors
    /// Returns `NetworkError` on failure.
    async fn get(&self, url: &Url) -> NetworkResult<Response>;

    /// Send a GET request with custom request headers.
    ///
    /// Used for APIs that require an auth header (e.g. Blockfrost's
    /// `project_id`). Implementations should otherwise behave identically
    /// to [`HttpClient::get`].
    ///
    /// # Errors
    /// Returns `NetworkError` on failure.
    async fn get_with_headers(
        &self,
        url: &Url,
        headers: &[(&str, &str)],
    ) -> NetworkResult<Response>;

    /// Send a POST request with a JSON body.
    ///
    /// # Errors
    /// Returns `NetworkError` on failure.
    async fn post_json(&self, url: &Url, body: &Value) -> NetworkResult<Response>;

    /// Send a POST request with a JSON body and custom headers.
    ///
    /// # Errors
    /// Returns `NetworkError` on failure.
    async fn post_json_with_headers(
        &self,
        url: &Url,
        body: &Value,
        headers: &[(&str, &str)],
    ) -> NetworkResult<Response>;
}

/// `reqwest`-backed HTTP client with optional Tor/SOCKS5 proxy.
pub struct ReqwestClient {
    inner: reqwest::Client,
}

impl ReqwestClient {
    /// Create a new client with the given timeout and privacy mode.
    ///
    /// # Errors
    /// Returns `NetworkError::Proxy` if the proxy URL is invalid.
    pub fn new(timeout: Duration, privacy: &PrivacyMode) -> NetworkResult<Self> {
        let mut builder = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent("Mozilla/5.0");

        if let PrivacyMode::Proxy { proxy_url } = privacy {
            let proxy = reqwest::Proxy::all(proxy_url.as_str())
                .map_err(|e| NetworkError::Proxy(format!("invalid proxy URL: {e}")))?;
            builder = builder.proxy(proxy);
            tracing::info!("network: routing through proxy {}", proxy_url);
        }

        let inner = builder
            .build()
            .map_err(|e| NetworkError::Connection(format!("client build: {e}")))?;

        Ok(Self { inner })
    }

    /// Create a direct (no-proxy) client with a 30s timeout.
    ///
    /// # Errors
    /// Returns `NetworkError::Connection` if the client cannot be built.
    pub fn direct() -> NetworkResult<Self> {
        Self::new(Duration::from_secs(30), &PrivacyMode::Direct)
    }

    /// Reject non-HTTPS URLs to prevent plaintext RPC traffic.
    ///
    /// Allows `http://localhost` and `http://127.0.0.1` for local
    /// development nodes.
    fn require_tls(url: &Url) -> NetworkResult<()> {
        match url.scheme() {
            "https" => Ok(()),
            "http" => {
                let host = url.host_str().unwrap_or("");
                if host == "localhost" || host == "127.0.0.1" || host == "[::1]" {
                    Ok(())
                } else {
                    Err(NetworkError::Connection(format!(
                        "insecure HTTP rejected for {host} — use HTTPS"
                    )))
                }
            }
            other => Err(NetworkError::Connection(format!(
                "unsupported URL scheme: {other}"
            ))),
        }
    }

    /// Map a `reqwest` error to our error type.
    fn map_err(e: &reqwest::Error) -> NetworkError {
        if e.is_timeout() {
            NetworkError::Timeout
        } else {
            NetworkError::Connection(e.to_string())
        }
    }
}

#[async_trait]
impl HttpClient for ReqwestClient {
    async fn get(&self, url: &Url) -> NetworkResult<Response> {
        self.get_with_headers(url, &[]).await
    }

    async fn get_with_headers(
        &self,
        url: &Url,
        headers: &[(&str, &str)],
    ) -> NetworkResult<Response> {
        Self::require_tls(url)?;
        let mut builder = self.inner.get(url.as_str());
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let resp = builder.send().await.map_err(|e| Self::map_err(&e))?;

        let status = resp.status().as_u16();
        let body = resp.bytes().await.map_err(|e| Self::map_err(&e))?.to_vec();

        if !(200..300).contains(&status) {
            let body_str = String::from_utf8_lossy(&body).chars().take(500).collect();
            return Err(NetworkError::Http {
                status,
                body: body_str,
            });
        }

        Ok(Response { status, body })
    }

    async fn post_json(&self, url: &Url, body: &Value) -> NetworkResult<Response> {
        self.post_json_with_headers(url, body, &[]).await
    }

    async fn post_json_with_headers(
        &self,
        url: &Url,
        body: &Value,
        headers: &[(&str, &str)],
    ) -> NetworkResult<Response> {
        Self::require_tls(url)?;
        let mut builder = self.inner.post(url.as_str()).json(body);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let resp = builder.send().await.map_err(|e| Self::map_err(&e))?;

        let status = resp.status().as_u16();
        let body_bytes = resp.bytes().await.map_err(|e| Self::map_err(&e))?.to_vec();

        if !(200..300).contains(&status) {
            let body_str = String::from_utf8_lossy(&body_bytes)
                .chars()
                .take(500)
                .collect();
            return Err(NetworkError::Http {
                status,
                body: body_str,
            });
        }

        Ok(Response {
            status,
            body: body_bytes,
        })
    }
}

// Rust guideline compliant 2026-05-02
