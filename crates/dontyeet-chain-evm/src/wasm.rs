//! In-browser EVM balance reads and transaction signing via JSON-RPC.
//!
//! Two entry points, both browser-only:
//!
//! - [`fetch_balance`] — Phase M.3.2.1 / M.3.2.9. Calls
//!   `eth_getBalance` directly from the WASM bundle, no server hop.
//! - [`send`] — Phase M.4.1. End-to-end client-side send pipeline:
//!   derive keypair → fetch nonce → fetch gas price → build →
//!   sign (via [`crate::signing`]) → broadcast.
//!
//! Lives outside the [`feature = "rpc"`] gate because `gloo_net` is
//! fundamentally a browser API. The whole module is `#[cfg]`-gated to
//! `wasm32` targets so it never enters native server builds.
//!
//! Mainnet RPC URLs (and EVM chain ids for EIP-155) are duplicated
//! from the [`chains`](crate::chains) factory functions (built-in
//! EVMs) and from `crates/dontyeet-ui/chain-registry.json` (catalog
//! L2s, served at `/chain-registry.json`); keep the three in sync.
//! We don't share a constant list because `chains/*.rs` is gated
//! behind `feature = "rpc"` and the catalog is parsed by the UI at
//! runtime, while this module needs to compile when `rpc` is *off*
//! (the typical browser consumer configuration is
//! `default-features = false`). A future phase will teach the
//! browser bundle about the catalog so this duplication can be
//! retired for catalog entries.
//!
//! Only mainnet is handled here. Testnet/devnet calls fall back to
//! the server-proxied Path A endpoint.

use serde::{Deserialize, Serialize};

use gloo_net::http::Request;

use dontyeet_crypto::paths;
use dontyeet_primitives::secret::Seed;
use dontyeet_primitives::{Amount, DontYeetWalletError, Result};

use crate::keys;
use crate::signing;

/// All EVM chains use 18 decimals for their native asset.
const EVM_DECIMALS: u8 = 18;

/// Gas limit for a simple ETH/native-coin transfer (no contract data).
///
/// Mirrors `crate::fees::SIMPLE_TRANSFER_GAS`; kept in step manually
/// because that constant lives behind `feature = "rpc"`.
const SIMPLE_TRANSFER_GAS: u64 = 21_000;

/// Hard cap on accepted gas price (500 gwei).
///
/// Mirrors `crate::fees::MAX_GAS_PRICE_WEI`. An RPC response above
/// this is treated as malicious or erroneous and rejected before
/// signing — protects users against a hostile RPC trying to drain
/// fees.
const MAX_GAS_PRICE_WEI: u128 = 500_000_000_000;

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Fetch the native balance for `address` on `chain_id` mainnet.
///
/// Calls the chain's public JSON-RPC endpoint directly from the
/// browser via [`gloo_net`], with no server roundtrip. Tries each
/// configured RPC URL in order and returns the first success. CORS
/// rejection, RPC outage, and transport errors are all reported as
/// [`DontYeetWalletError::Network`].
///
/// # Errors
/// - [`DontYeetWalletError::Network`] if all RPC URLs fail or the chain
///   isn't a known EVM chain.
/// - [`DontYeetWalletError::Chain`] if the response can't be parsed as a
///   hex `u128` wei amount.
pub async fn fetch_balance(chain_id: &str, address: &str) -> Result<Amount> {
    let urls = mainnet_rpc_urls(chain_id).ok_or_else(|| {
        DontYeetWalletError::Network(format!("no in-browser RPC URLs configured for {chain_id}"))
    })?;

    let hex_str = rpc_call(
        urls,
        "eth_getBalance",
        serde_json::json!([address, "latest"]),
    )
    .await?;
    let wei = parse_hex_u128(&hex_str)?;
    Ok(Amount::from_raw(wei, EVM_DECIMALS))
}

/// Build the signed raw EVM transaction without broadcasting.
///
/// Pipeline: derive keypair from `seed` (BIP-44 `m/44'/60'/0'/0/0`)
/// → fetch nonce (`eth_getTransactionCount`) → fetch gas price
/// (`eth_gasPrice`, capped at 500 gwei) → RLP-encode the EIP-155
/// unsigned form → secp256k1 sign via [`crate::signing::sign_legacy_tx`].
/// Returns the raw signed transaction bytes ready for
/// `eth_sendRawTransaction`.
///
/// Splitting build from broadcast lets the orchestration layer hash
/// the signed bytes for deduplication caching, fee-bump retries, and
/// (eventually) offline signing.
///
/// # Errors
/// - [`DontYeetWalletError::Network`] for any RPC failure (CORS, RPC
///   outage, fetch rejection) or unknown `chain_id`.
/// - [`DontYeetWalletError::Validation`] if `to` lacks a `0x` prefix, isn't
///   valid hex, `amount_raw` doesn't parse as a `u128`, or the RPC
///   returns a gas price above [`MAX_GAS_PRICE_WEI`].
/// - [`DontYeetWalletError::Crypto`] if BIP-44 derivation or ECDSA signing
///   fails (or post-sign verification mismatches).
pub async fn build_signed_payload(
    chain_id: &str,
    seed: &Seed,
    to: &str,
    amount_raw: &str,
) -> Result<Vec<u8>> {
    let urls = mainnet_rpc_urls(chain_id).ok_or_else(|| {
        DontYeetWalletError::Network(format!("no in-browser RPC URLs configured for {chain_id}"))
    })?;
    let evm_chain_id = mainnet_evm_chain_id(chain_id).ok_or_else(|| {
        DontYeetWalletError::Network(format!("no EIP-155 chain id configured for {chain_id}"))
    })?;

    // 1. Derive the sender's keypair (no I/O).
    let keypair = keys::derive_keypair(seed, paths::ETHEREUM)?;
    let private_key = keypair
        .private_key()
        .ok_or_else(|| DontYeetWalletError::Crypto("derived keypair has no private key".into()))?;

    // 2. Allocate a nonce against `pending` so multiple sends
    //    in flight at once don't collide.
    let nonce_hex = rpc_call(
        urls,
        "eth_getTransactionCount",
        serde_json::json!([keypair.address.as_str(), "pending"]),
    )
    .await?;
    let nonce = parse_hex_u64(&nonce_hex)?;

    // 3. Estimate gas price and clamp.
    let gas_price_hex = rpc_call(urls, "eth_gasPrice", serde_json::json!([])).await?;
    let gas_price = parse_hex_u128(&gas_price_hex)?;
    if gas_price == 0 {
        return Err(DontYeetWalletError::Validation(
            "RPC returned zero gas price".into(),
        ));
    }
    if gas_price > MAX_GAS_PRICE_WEI {
        return Err(DontYeetWalletError::Validation(format!(
            "gas price {gas_price} wei exceeds safety cap of {MAX_GAS_PRICE_WEI} wei (500 gwei)"
        )));
    }

    // 4. Parse the destination address.
    let to_hex = to
        .strip_prefix("0x")
        .or_else(|| to.strip_prefix("0X"))
        .ok_or_else(|| DontYeetWalletError::Validation("address missing 0x prefix".into()))?;
    let to_bytes = hex::decode(to_hex).map_err(|e| DontYeetWalletError::Validation(e.to_string()))?;

    // 5. Parse the amount (decimal wei string).
    let amount_wei: u128 = amount_raw
        .parse()
        .map_err(|e| DontYeetWalletError::Validation(format!("invalid amount '{amount_raw}': {e}")))?;

    // 6. Build + sign in a single shot via the shared `signing` module.
    let unsigned = signing::rlp_encode_unsigned(
        nonce,
        gas_price,
        SIMPLE_TRANSFER_GAS,
        &to_bytes,
        amount_wei,
        &[],
        evm_chain_id,
    );
    signing::sign_legacy_tx(&unsigned, private_key, evm_chain_id)
}

/// Broadcast a pre-signed raw EVM transaction.
///
/// Submits via `eth_sendRawTransaction` against the chain's mainnet
/// RPC pool and returns the resulting transaction hash (`0x...`).
///
/// # Errors
/// - [`DontYeetWalletError::Network`] for unknown `chain_id` or any RPC
///   transport / broadcast failure.
pub async fn broadcast_signed(chain_id: &str, signed: &[u8]) -> Result<String> {
    let urls = mainnet_rpc_urls(chain_id).ok_or_else(|| {
        DontYeetWalletError::Network(format!("no in-browser RPC URLs configured for {chain_id}"))
    })?;
    let raw_hex = format!("0x{}", hex::encode(signed));
    rpc_call(urls, "eth_sendRawTransaction", serde_json::json!([raw_hex])).await
}

/// Sign and broadcast a native EVM transfer entirely client-side.
///
/// Convenience wrapper around [`build_signed_payload`] +
/// [`broadcast_signed`]. The orchestration layer in the UI calls the
/// two halves directly so it can dedup retries between them; new
/// callers should prefer the split form unless they explicitly need
/// the single-shot path.
///
/// # Errors
/// Same as [`build_signed_payload`] and [`broadcast_signed`].
pub async fn send(chain_id: &str, seed: &Seed, to: &str, amount_raw: &str) -> Result<String> {
    let signed = build_signed_payload(chain_id, seed, to, amount_raw).await?;
    broadcast_signed(chain_id, &signed).await
}

// ---------------------------------------------------------------------------
// Chain id / RPC URL tables
// ---------------------------------------------------------------------------

/// Mainnet RPC URLs for every EVM chain the wallet supports.
///
/// First five (`ethereum` … `sonic`) are the built-in plugins from
/// [`crate::chains`]; the remaining five are catalog L2s registered
/// via `crates/dontyeet-ui/chain-registry.json`. Returns `None` only
/// for unknown chain ids — callers fall back to the server-proxied
/// path then.
fn mainnet_rpc_urls(chain_id: &str) -> Option<&'static [&'static str]> {
    Some(match chain_id {
        // Built-in EVMs (chains/*.rs).
        "ethereum" => &[
            "https://eth.llamarpc.com",
            "https://ethereum-rpc.publicnode.com",
        ],
        "polygon" => &[
            "https://polygon-rpc.com",
            "https://polygon-mainnet.public.blastapi.io",
        ],
        "bnb" => &["https://bsc-dataseed1.bnbchain.org"],
        "avalanche" => &["https://api.avax.network/ext/bc/C/rpc"],
        "sonic" => &["https://rpc.soniclabs.com"],
        // Catalog L2s (crates/dontyeet-ui/chain-registry.json).
        "base" => &["https://mainnet.base.org"],
        "arbitrum" => &["https://arb1.arbitrum.io/rpc"],
        "optimism" => &["https://mainnet.optimism.io"],
        "zksync" => &["https://mainnet.era.zksync.io"],
        "linea" => &["https://rpc.linea.build"],
        _ => return None,
    })
}

/// EIP-155 chain ID for each supported EVM chain on mainnet.
///
/// Values match the `evm_chain_id_mainnet` field of each
/// [`EvmChainConfig`](crate::config::EvmChainConfig) (built-in
/// chains) and the `evm_chain_id` of each catalog entry.
fn mainnet_evm_chain_id(chain_id: &str) -> Option<u64> {
    Some(match chain_id {
        "ethereum" => 1,
        "polygon" => 137,
        "bnb" => 56,
        "avalanche" => 43_114,
        "sonic" => 146,
        "base" => 8_453,
        "arbitrum" => 42_161,
        "optimism" => 10,
        "zksync" => 324,
        "linea" => 59_144,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// JSON-RPC plumbing
// ---------------------------------------------------------------------------

/// Make a JSON-RPC call against the first URL that answers.
///
/// Tries each URL in order and returns the first successful
/// `result` field. If every URL fails, returns the last error —
/// typically a [`DontYeetWalletError::Network`] describing CORS / status
/// / parse failure.
async fn rpc_call(urls: &[&str], method: &str, params: serde_json::Value) -> Result<String> {
    let body = JsonRpcRequest {
        jsonrpc: "2.0",
        id: 1,
        method,
        params,
    };

    let mut last_err: Option<DontYeetWalletError> = None;
    for url in urls {
        match try_one(url, &body).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_err = Some(e);
            }
        }
    }

    Err(last_err
        .unwrap_or_else(|| DontYeetWalletError::Network("no RPC URLs returned a response".into())))
}

/// One JSON-RPC POST against a single URL.
async fn try_one(url: &str, body: &JsonRpcRequest<'_>) -> Result<String> {
    let request = Request::post(url)
        .json(body)
        .map_err(|e| DontYeetWalletError::Network(format!("request build failed: {e}")))?;

    let resp = request
        .send()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("RPC fetch failed: {e}")))?;

    if !resp.ok() {
        return Err(DontYeetWalletError::Network(format!(
            "RPC {url} returned status {}",
            resp.status()
        )));
    }

    let parsed: JsonRpcResponse = resp
        .json()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("RPC body parse: {e}")))?;

    if let Some(err) = parsed.error {
        return Err(DontYeetWalletError::Network(format!(
            "RPC error: {} (code {})",
            err.message, err.code
        )));
    }

    parsed
        .result
        .ok_or_else(|| DontYeetWalletError::Network("RPC response missing `result`".into()))
}

/// Parse a `0x`-prefixed hex string to `u128`.
fn parse_hex_u128(hex_str: &str) -> Result<u128> {
    let stripped = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    if stripped.is_empty() {
        return Ok(0);
    }
    u128::from_str_radix(stripped, 16)
        .map_err(|e| DontYeetWalletError::Chain(format!("hex parse error: {e}")))
}

/// Parse a `0x`-prefixed hex string to `u64`.
fn parse_hex_u64(hex_str: &str) -> Result<u64> {
    let stripped = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    if stripped.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(stripped, 16)
        .map_err(|e| DontYeetWalletError::Chain(format!("hex parse error: {e}")))
}

#[derive(Serialize)]
struct JsonRpcRequest<'a> {
    jsonrpc: &'a str,
    id: u32,
    method: &'a str,
    params: serde_json::Value,
}

#[derive(Deserialize)]
struct JsonRpcResponse {
    result: Option<String>,
    error: Option<JsonRpcError>,
}

#[derive(Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

// Rust guideline compliant 2026-02-21
