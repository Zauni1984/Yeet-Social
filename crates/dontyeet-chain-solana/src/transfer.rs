//! Server-side Solana native SOL transfer.
//!
//! Decodes sender / recipient addresses, fetches a recent blockhash
//! from the Solana JSON-RPC, and hands the lot to
//! [`signing::build_signed_transfer`] for the pure-crypto pipeline
//! (v0 message construction, Ed25519 signing, wire-format
//! assembly). Phase M.4.5 moved everything after the network call
//! into the always-on [`crate::signing`] module so the in-browser
//! send pipeline goes through the same code.

use std::collections::HashMap;

use url::Url;

use dontyeet_network::{HttpClient, ReqwestClient};
use dontyeet_primitives::address::Address;
use dontyeet_primitives::amount::Amount;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::PrivateKey;

use crate::signing;

// ---- RPC response shapes ----

#[derive(serde::Deserialize)]
struct BlockhashRpcResponse {
    result: BlockhashRpcResult,
}

#[derive(serde::Deserialize)]
struct BlockhashRpcResult {
    value: BlockhashValue,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlockhashValue {
    blockhash: String,
}

// ---- Public API ----

/// Build and sign a native-SOL transfer transaction.
///
/// Decodes `from` and `to` addresses, fetches a recent blockhash via
/// JSON-RPC, then delegates to [`signing::build_signed_transfer`]
/// for the v0 message + Ed25519 sign + wire-format assembly.
///
/// # Errors
/// Returns [`DontYeetWalletError`] if the API URL list is missing for
/// `network`, the JSON-RPC call fails, address decoding fails, or
/// signing fails.
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
    let lamports = u64::try_from(amount.raw())
        .map_err(|_| DontYeetWalletError::Validation("amount too large for SOL transfer".into()))?;

    let from_pubkey = signing::decode_address(from.as_str())?;
    let to_pubkey = signing::decode_address(to.as_str())?;

    let blockhash = fetch_recent_blockhash(api_urls, network).await?;

    signing::build_signed_transfer(&from_pubkey, &to_pubkey, &blockhash, lamports, private_key)
}

// ---- RPC plumbing ----

/// Fetch the latest blockhash from the Solana JSON-RPC API.
async fn fetch_recent_blockhash(
    api_urls: &HashMap<NetworkId, Vec<Url>>,
    network: &NetworkId,
) -> Result<[u8; 32]> {
    let urls = api_urls
        .get(network)
        .ok_or_else(|| DontYeetWalletError::NotFound(format!("no API URLs for {network}")))?;
    let base = urls
        .first()
        .ok_or_else(|| DontYeetWalletError::NotFound("API URL list is empty".into()))?;

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getLatestBlockhash",
        "params": []
    });

    let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
    let response = client
        .post_json(base, &body)
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("getLatestBlockhash: {e}")))?;

    let rpc_resp: BlockhashRpcResponse = response
        .json()
        .map_err(|e| DontYeetWalletError::Network(format!("blockhash parse: {e}")))?;

    let hash_bytes = bs58::decode(&rpc_resp.result.value.blockhash)
        .into_vec()
        .map_err(|e| DontYeetWalletError::Chain(format!("blockhash decode: {e}")))?;

    <[u8; 32]>::try_from(hash_bytes.as_slice())
        .map_err(|_| DontYeetWalletError::Chain("blockhash is not 32 bytes".into()))
}

// Rust guideline compliant 2026-02-21
