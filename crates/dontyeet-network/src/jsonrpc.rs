//! JSON-RPC 2.0 client for blockchain node communication.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use url::Url;

use crate::client::HttpClient;
use crate::error::{NetworkError, NetworkResult};

/// A JSON-RPC 2.0 request.
#[derive(Debug, Serialize)]
pub struct RpcRequest {
    jsonrpc: &'static str,
    method: String,
    params: Value,
    id: u64,
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Deserialize)]
pub struct RpcResponse<T> {
    /// Successful result (mutually exclusive with `error`).
    pub result: Option<T>,
    /// Error payload (mutually exclusive with `result`).
    pub error: Option<RpcErrorPayload>,
    /// Request ID echo.
    pub id: Option<u64>,
}

/// JSON-RPC error object.
#[derive(Debug, Deserialize)]
pub struct RpcErrorPayload {
    /// Numeric error code.
    pub code: i64,
    /// Human-readable error message.
    pub message: String,
}

/// JSON-RPC client that wraps any [`HttpClient`].
pub struct RpcClient<C: HttpClient> {
    client: C,
    url: Url,
    next_id: AtomicU64,
}

impl<C: HttpClient> RpcClient<C> {
    /// Create a new RPC client for the given endpoint.
    #[must_use]
    pub fn new(client: C, url: Url) -> Self {
        Self {
            client,
            url,
            next_id: AtomicU64::new(1),
        }
    }

    /// The endpoint URL.
    #[must_use]
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Call a single JSON-RPC method.
    ///
    /// # Errors
    /// Returns `NetworkError::Rpc` if the node returns an error, or other
    /// `NetworkError` variants on transport failure.
    pub async fn call<T: DeserializeOwned>(&self, method: &str, params: Value) -> NetworkResult<T> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        let request = RpcRequest {
            jsonrpc: "2.0",
            method: method.to_string(),
            params,
            id,
        };

        let body = serde_json::to_value(&request)
            .map_err(|e| NetworkError::Deserialize(format!("serialize request: {e}")))?;

        let response = self.client.post_json(&self.url, &body).await?;
        let rpc_resp: RpcResponse<T> = response.json()?;

        if let Some(err) = rpc_resp.error {
            return Err(NetworkError::Rpc {
                code: err.code,
                message: err.message,
            });
        }

        rpc_resp.result.ok_or_else(|| {
            NetworkError::Deserialize("RPC response has neither result nor error".into())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_request_serializes_correctly() {
        let req = RpcRequest {
            jsonrpc: "2.0",
            method: "eth_getBalance".into(),
            params: serde_json::json!(["0xabc", "latest"]),
            id: 1,
        };
        let json = serde_json::to_value(&req).expect("serialize");
        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["method"], "eth_getBalance");
        assert_eq!(json["id"], 1);
    }

    #[test]
    fn rpc_error_response_parses() {
        let json =
            r#"{"jsonrpc":"2.0","error":{"code":-32601,"message":"Method not found"},"id":1}"#;
        let resp: RpcResponse<Value> = serde_json::from_str(json).expect("parse");
        assert!(resp.result.is_none());
        let err = resp.error.expect("should have error");
        assert_eq!(err.code, -32601);
    }
}

// Rust guideline compliant 2026-05-02
