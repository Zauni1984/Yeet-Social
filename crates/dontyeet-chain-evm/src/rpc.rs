//! Internal RPC helper for EVM chains.
//!
//! Provides a thin wrapper that creates a temporary [`RpcClient`] per call,
//! rotating through available endpoints in round-robin order. If one
//! endpoint fails, the next is tried until all have been exhausted.

use std::sync::atomic::{AtomicUsize, Ordering};

use serde::de::DeserializeOwned;
use serde_json::Value;
use url::Url;

use dontyeet_network::{ReqwestClient, RpcClient};
use dontyeet_primitives::error::{DontYeetWalletError, Result};

/// Round-robin counter shared across all calls within this process.
static ENDPOINT_INDEX: AtomicUsize = AtomicUsize::new(0);

/// Call an EVM JSON-RPC method, rotating through endpoints.
///
/// Tries each URL in round-robin order. If a call fails, the next
/// endpoint is tried. Returns the first successful result, or the
/// last error if all endpoints fail.
///
/// # Errors
/// Returns `DontYeetWalletError::Network` if all RPC URLs fail or the client
/// cannot be constructed.
pub async fn rpc_call<T: DeserializeOwned>(urls: &[Url], method: &str, params: Value) -> Result<T> {
    if urls.is_empty() {
        return Err(DontYeetWalletError::Network(
            "no RPC URLs configured for this network".into(),
        ));
    }

    let start = ENDPOINT_INDEX.fetch_add(1, Ordering::Relaxed);
    let mut last_err = None;

    for i in 0..urls.len() {
        let idx = (start.wrapping_add(i)) % urls.len();
        let url = &urls[idx];

        let client = match ReqwestClient::direct() {
            Ok(c) => c,
            Err(e) => {
                last_err = Some(DontYeetWalletError::Network(e.to_string()));
                continue;
            }
        };

        let rpc = RpcClient::new(client, url.clone());
        match rpc.call(method, params.clone()).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                tracing::warn!(
                    endpoint = %url,
                    method,
                    error = %e,
                    "RPC call failed, trying next endpoint"
                );
                last_err = Some(DontYeetWalletError::Network(e.to_string()));
            }
        }
    }

    Err(last_err.unwrap_or_else(|| DontYeetWalletError::Network("all RPC endpoints exhausted".into())))
}

/// Parse a `0x`-prefixed hex string to `u128`.
///
/// # Errors
/// Returns `DontYeetWalletError::Chain` if the hex cannot be parsed.
pub fn parse_hex_u128(hex_str: &str) -> Result<u128> {
    let stripped = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    if stripped.is_empty() {
        return Ok(0);
    }
    u128::from_str_radix(stripped, 16)
        .map_err(|e| DontYeetWalletError::Chain(format!("hex parse error: {e}")))
}

/// Parse a `0x`-prefixed hex string to `u64`.
///
/// # Errors
/// Returns `DontYeetWalletError::Chain` if the hex cannot be parsed.
pub fn parse_hex_u64(hex_str: &str) -> Result<u64> {
    let stripped = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    if stripped.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(stripped, 16)
        .map_err(|e| DontYeetWalletError::Chain(format!("hex parse error: {e}")))
}

// Rust guideline compliant 2026-05-02
