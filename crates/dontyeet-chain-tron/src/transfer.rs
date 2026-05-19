//! Server-side TRON native TRX transfer.
//!
//! Asks `TronGrid` to build the unsigned transaction via
//! `POST /wallet/createtransaction`, signs the returned `raw_data_hex`
//! locally via [`crate::signing::sign_and_attach`], and returns the
//! signed transaction JSON bytes ready for the broadcaster. Phase
//! M.4.3 moved the SHA-256 + ECDSA + JSON-splice core into the
//! always-on [`crate::signing`] module so the in-browser send
//! pipeline goes through the same code.

use std::collections::HashMap;

use url::Url;

use dontyeet_network::{HttpClient, ReqwestClient};
use dontyeet_primitives::address::Address;
use dontyeet_primitives::amount::Amount;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::PrivateKey;

use crate::signing;

/// Build and sign a native TRX transfer.
///
/// Calls `POST /wallet/createtransaction` on `TronGrid` to have the node
/// build the unsigned transaction, hands the resulting JSON envelope
/// to [`signing::sign_and_attach`], and returns the signed JSON bytes
/// for the broadcaster. Uses `visible=true` so `Base58Check` addresses
/// are passed and returned directly.
///
/// # Errors
/// Returns [`DontYeetWalletError`] if the API call fails, the response is
/// malformed (missing `raw_data_hex`), `amount.raw()` is too large
/// for `u64`, or signing fails.
#[expect(
    clippy::implicit_hasher,
    reason = "callers always pass HashMap with default RandomState; generic hasher would not improve API ergonomics"
)]
pub async fn build_signed_transfer(
    api_urls: &HashMap<NetworkId, Vec<Url>>,
    from: &Address,
    to: &Address,
    amount: &Amount,
    private_key: &PrivateKey,
    network: &NetworkId,
) -> Result<Vec<u8>> {
    let urls = api_urls
        .get(network)
        .ok_or_else(|| DontYeetWalletError::NotFound(format!("no API URLs for {network}")))?;
    let base = urls
        .first()
        .ok_or_else(|| DontYeetWalletError::NotFound("API URL list is empty".into()))?;

    let amount_u64 = u64::try_from(amount.raw())
        .map_err(|_| DontYeetWalletError::Validation("amount too large for TRON transfer".into()))?;

    // 1. Ask the node to build the unsigned transaction.
    let create_url = base
        .join("/wallet/createtransaction")
        .map_err(|e| DontYeetWalletError::Network(format!("URL join error: {e}")))?;

    let body = serde_json::json!({
        "owner_address": from.as_str(),
        "to_address": to.as_str(),
        "amount": amount_u64,
        "visible": true
    });

    let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
    let response = client
        .post_json(&create_url, &body)
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("createtransaction: {e}")))?;

    let mut tx_json: serde_json::Value = response
        .json()
        .map_err(|e| DontYeetWalletError::Network(format!("tx response parse: {e}")))?;

    // 2. Extract raw_data_hex.
    let raw_data_hex = tx_json
        .get("raw_data_hex")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            DontYeetWalletError::Chain("missing raw_data_hex in createtransaction response".into())
        })?
        .to_owned();

    // 3. Sign + splice via the shared signing core.
    signing::sign_and_attach(&mut tx_json, &raw_data_hex, private_key)?;

    // 4. Return signed JSON bytes for broadcasting.
    serde_json::to_vec(&tx_json).map_err(|e| DontYeetWalletError::Chain(format!("JSON serialize: {e}")))
}

// Rust guideline compliant 2026-02-21
