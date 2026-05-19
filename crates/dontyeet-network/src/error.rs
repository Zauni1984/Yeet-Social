//! Network-specific error types.

/// Errors originating from network operations.
#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    /// HTTP request returned a non-success status.
    #[error("HTTP {status}: {body}")]
    Http {
        /// HTTP status code.
        status: u16,
        /// Response body (truncated for safety).
        body: String,
    },

    /// Request timed out.
    #[error("request timed out")]
    Timeout,

    /// DNS/TCP connection failed.
    #[error("connection: {0}")]
    Connection(String),

    /// JSON-RPC error response from the node.
    #[error("RPC error {code}: {message}")]
    Rpc {
        /// JSON-RPC error code.
        code: i64,
        /// Error message from the node.
        message: String,
    },

    /// Local rate limiter blocked the request.
    #[error("rate limited — too many requests")]
    RateLimited,

    /// Response deserialization failed.
    #[error("deserialize: {0}")]
    Deserialize(String),

    /// Proxy or Tor connection failed.
    #[error("proxy: {0}")]
    Proxy(String),
}

/// Convenience alias.
pub type NetworkResult<T> = std::result::Result<T, NetworkError>;

impl From<NetworkError> for dontyeet_primitives::DontYeetWalletError {
    fn from(e: NetworkError) -> Self {
        Self::Network(e.to_string())
    }
}

// Rust guideline compliant 2026-05-02
