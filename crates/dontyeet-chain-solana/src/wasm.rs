//! In-browser Solana balance reads and transaction signing.
//!
//! Two entry points, both browser-only:
//!
//! - [`fetch_balance`] — Phase M.3.2.5. Calls the `getBalance`
//!   JSON-RPC method directly from the WASM bundle, no server hop.
//! - [`send`] — Phase M.4.5. End-to-end client-side send pipeline:
//!   derive keypair → derive sender pubkey → decode recipient →
//!   fetch a recent blockhash via `getLatestBlockhash` → call
//!   [`crate::signing::build_signed_transfer`] → broadcast via
//!   `sendTransaction` with a Base64-encoded blob.
//!
//! Lives outside the [`feature = "rpc"`] gate; `#[cfg]`-gated to
//! `wasm32` targets so server builds stay unaffected.
//!
//! ## CORS / rate limits
//!
//! The public `https://api.mainnet-beta.solana.com` endpoint is
//! heavily rate-limited and may reject browser origins under load.
//! Failures here flow through to the server-proxied fallback in
//! [`balances`](crate::balance) (for reads) or
//! `wallet::send::try_direct` (for sends), so the UX degrades
//! gracefully if the public RPC throttles us.

use data_encoding::BASE64;
use serde::Deserialize;

use gloo_net::http::Request;

use dontyeet_crypto::{Bip44Deriver, paths};
use dontyeet_primitives::secret::Seed;
use dontyeet_primitives::{Amount, DontYeetWalletError, Result};

use crate::keys::ed25519_pubkey;
use crate::signing;

/// 1 SOL = `1_000_000_000` lamports — 9 decimal places.
const SOL_DECIMALS: u8 = 9;

/// Solana mainnet-beta JSON-RPC endpoint.
///
/// Matches the entry in [`config::default_sol_config`](crate::config)
/// — keep in sync.
const SOLANA_MAINNET_RPC: &str = "https://api.mainnet-beta.solana.com";

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Fetch the on-chain SOL balance for `address` on mainnet-beta.
///
/// Calls the `getBalance` JSON-RPC method against the public Solana
/// node and reads `result.value`, expressed in lamports
/// (1 SOL = 1e9 lamports).
///
/// # Errors
/// - [`DontYeetWalletError::Network`] if `chain_id` isn't `"solana"`, the
///   HTTP call fails (CORS rejection, rate limit, transport error),
///   or the body can't be parsed.
pub async fn fetch_balance(chain_id: &str, address: &str) -> Result<Amount> {
    if chain_id != "solana" {
        return Err(DontYeetWalletError::Network(format!(
            "in-browser Solana fetcher does not handle {chain_id}"
        )));
    }

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getBalance",
        "params": [address]
    });

    let parsed: GetBalanceResponse = post_json(SOLANA_MAINNET_RPC, &body).await?;

    Ok(Amount::from_raw(
        u128::from(parsed.result.value),
        SOL_DECIMALS,
    ))
}

/// Build a signed Solana transfer transaction without broadcasting.
///
/// Returns the raw signed-transaction bytes ready for Base64-encoding
/// into `sendTransaction`.
///
/// # Errors
/// Same shape as the previous `send`, minus the broadcast step.
pub async fn build_signed_payload(
    chain_id: &str,
    seed: &Seed,
    to: &str,
    amount_raw: &str,
) -> Result<Vec<u8>> {
    if chain_id != "solana" {
        return Err(DontYeetWalletError::Network(format!(
            "in-browser Solana sender does not handle {chain_id}"
        )));
    }

    let private_key = Bip44Deriver::derive(seed, paths::SOLANA)?;
    let from_pubkey_bytes = ed25519_pubkey(&private_key)?;
    let from_pubkey: [u8; 32] = from_pubkey_bytes
        .as_slice()
        .try_into()
        .map_err(|_| DontYeetWalletError::Crypto("ed25519 pubkey is not 32 bytes".into()))?;

    let to_pubkey = signing::decode_address(to)?;
    let lamports: u64 = amount_raw
        .parse()
        .map_err(|e| DontYeetWalletError::Validation(format!("invalid amount '{amount_raw}': {e}")))?;

    let blockhash = fetch_recent_blockhash().await?;

    signing::build_signed_transfer(&from_pubkey, &to_pubkey, &blockhash, lamports, &private_key)
}

/// Broadcast a pre-signed Solana transaction.
///
/// Base64-encodes the bytes and submits via `sendTransaction`. Returns
/// the broadcast tx signature.
///
/// # Errors
/// - [`DontYeetWalletError::Network`] for unknown `chain_id` or any RPC failure.
pub async fn broadcast_signed(chain_id: &str, signed: &[u8]) -> Result<String> {
    if chain_id != "solana" {
        return Err(DontYeetWalletError::Network(format!(
            "in-browser Solana broadcaster does not handle {chain_id}"
        )));
    }
    let encoded_tx = BASE64.encode(signed);
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "sendTransaction",
        "params": [encoded_tx, { "encoding": "base64" }]
    });
    let resp: SendTxResponse = post_json(SOLANA_MAINNET_RPC, &body).await?;
    Ok(resp.result)
}

/// Sign and broadcast a native-SOL transfer entirely client-side.
///
/// Convenience wrapper around [`build_signed_payload`] +
/// [`broadcast_signed`].
///
/// # Errors
/// Same as [`build_signed_payload`] and [`broadcast_signed`].
pub async fn send(chain_id: &str, seed: &Seed, to: &str, amount_raw: &str) -> Result<String> {
    let signed = build_signed_payload(chain_id, seed, to, amount_raw).await?;
    broadcast_signed(chain_id, &signed).await
}

// ---------------------------------------------------------------------------
// HTTP plumbing
// ---------------------------------------------------------------------------

/// POST a JSON-RPC body and decode the response into `T`.
///
/// Centralizes the `gloo_net` error-mapping shape used by every Solana
/// JSON-RPC call this module makes.
async fn post_json<T: for<'de> Deserialize<'de>>(url: &str, body: &serde_json::Value) -> Result<T> {
    let request = Request::post(url)
        .json(body)
        .map_err(|e| DontYeetWalletError::Network(format!("request build failed: {e}")))?;

    let resp = request
        .send()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("POST {url} failed: {e}")))?;

    if !resp.ok() {
        return Err(DontYeetWalletError::Network(format!(
            "{url} returned status {}",
            resp.status()
        )));
    }

    resp.json()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("{url} body parse: {e}")))
}

/// Fetch the latest blockhash via `getLatestBlockhash`.
async fn fetch_recent_blockhash() -> Result<[u8; 32]> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getLatestBlockhash",
        "params": []
    });
    let rpc: BlockhashRpcResponse = post_json(SOLANA_MAINNET_RPC, &body).await?;
    let hash_bytes = bs58::decode(&rpc.result.value.blockhash)
        .into_vec()
        .map_err(|e| DontYeetWalletError::Chain(format!("blockhash decode: {e}")))?;
    <[u8; 32]>::try_from(hash_bytes.as_slice())
        .map_err(|_| DontYeetWalletError::Chain("blockhash is not 32 bytes".into()))
}

// ---------------------------------------------------------------------------
// Response shapes
// ---------------------------------------------------------------------------

/// Outer JSON-RPC envelope for `getBalance`.
#[derive(Deserialize)]
struct GetBalanceResponse {
    result: BalanceResult,
}

/// Inner `getBalance` result containing the lamport balance.
#[derive(Deserialize)]
struct BalanceResult {
    value: u64,
}

/// Outer JSON-RPC envelope for `getLatestBlockhash`.
#[derive(Deserialize)]
struct BlockhashRpcResponse {
    result: BlockhashRpcResult,
}

/// Inner `getLatestBlockhash` result.
#[derive(Deserialize)]
struct BlockhashRpcResult {
    value: BlockhashValue,
}

/// `getLatestBlockhash` blockhash payload (camelCase on the wire).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlockhashValue {
    blockhash: String,
}

/// Outer JSON-RPC envelope for `sendTransaction`.
#[derive(Deserialize)]
struct SendTxResponse {
    result: String,
}

// Rust guideline compliant 2026-02-21
