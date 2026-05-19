//! In-browser Algorand balance reads and transaction signing.
//!
//! Two entry points, both browser-only:
//!
//! - [`fetch_balance`] — Phase M.3.2.6. Calls
//!   `GET {base}/v2/accounts/{address}` directly from the WASM
//!   bundle, no server hop.
//! - [`send`] — Phase M.4.6. End-to-end client-side send pipeline:
//!   derive keypair → derive sender pubkey → fetch suggested
//!   params → call [`crate::signing::build_signed_payment`] →
//!   broadcast via `POST /v2/transactions` with the raw msgpack
//!   body and `Content-Type: application/x-binary`.
//!
//! Lives outside the [`feature = "rpc"`] gate; `#[cfg]`-gated to
//! `wasm32` targets so server builds stay unaffected.
//!
//! Mainnet only — testnet calls fall back to the server-proxied
//! Path A endpoint.

use serde::Deserialize;

use gloo_net::http::Request;
use js_sys::Uint8Array;
use wasm_bindgen::JsValue;

use dontyeet_crypto::{Bip44Deriver, paths};
use dontyeet_primitives::secret::Seed;
use dontyeet_primitives::{Amount, DontYeetWalletError, Result};

use crate::keys::ed25519_pubkey;
use crate::signing::{self, PaymentParams};

/// 1 ALGO = `1_000_000` microAlgos — 6 decimal places.
const ALGO_DECIMALS: u8 = 6;

/// Nodely Algod v2 mainnet REST endpoint.
///
/// Matches the entry in [`config::default_algo_config`](crate::config)
/// — keep in sync.
const ALGOD_MAINNET: &str = "https://mainnet-api.4160.nodely.dev";

/// Validity window: `last_valid = first_valid + this offset`. Mirrors
/// the server-side `transfer::VALIDITY_ROUNDS`.
const VALIDITY_ROUNDS: u64 = 1000;

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Fetch the on-chain ALGO balance for `address` on mainnet.
///
/// Calls `GET {base}/v2/accounts/{address}` and reads the `amount`
/// field, expressed in microAlgos.
///
/// # Errors
/// - [`DontYeetWalletError::Network`] if `chain_id` isn't `"algorand"`,
///   the HTTP call fails (CORS rejection, transport error, non-2xx
///   status), or the body can't be parsed.
pub async fn fetch_balance(chain_id: &str, address: &str) -> Result<Amount> {
    if chain_id != "algorand" {
        return Err(DontYeetWalletError::Network(format!(
            "in-browser Algorand fetcher does not handle {chain_id}"
        )));
    }

    // Algorand addresses are 58-char base32, URL-safe — direct
    // interpolation is fine.
    let url = format!("{ALGOD_MAINNET}/v2/accounts/{address}");

    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("account fetch failed: {e}")))?;

    if !resp.ok() {
        return Err(DontYeetWalletError::Network(format!(
            "{url} returned status {}",
            resp.status()
        )));
    }

    let info: AccountInfo = resp
        .json()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("account info parse: {e}")))?;

    Ok(Amount::from_raw(u128::from(info.amount), ALGO_DECIMALS))
}

/// Build a signed Algorand Payment without broadcasting.
///
/// Returns the raw signed msgpack bytes ready for `POST /v2/transactions`.
///
/// # Errors
/// Same shape as the previous `send`, minus the broadcast step.
pub async fn build_signed_payload(
    chain_id: &str,
    seed: &Seed,
    to: &str,
    amount_raw: &str,
) -> Result<Vec<u8>> {
    if chain_id != "algorand" {
        return Err(DontYeetWalletError::Network(format!(
            "in-browser Algorand sender does not handle {chain_id}"
        )));
    }

    let private_key = Bip44Deriver::derive(seed, paths::ALGORAND)?;
    let sender_bytes = ed25519_pubkey(&private_key)?;
    let sender: [u8; 32] = sender_bytes
        .as_slice()
        .try_into()
        .map_err(|_| DontYeetWalletError::Crypto("ed25519 pubkey is not 32 bytes".into()))?;

    let receiver = signing::address_to_pubkey(to)?;
    let micro_algos: u64 = amount_raw
        .parse()
        .map_err(|e| DontYeetWalletError::Validation(format!("invalid amount '{amount_raw}': {e}")))?;

    let params = fetch_tx_params().await?;
    let genesis_hash_bytes = data_encoding::BASE64
        .decode(params.genesis_hash.as_bytes())
        .map_err(|e| DontYeetWalletError::Chain(format!("genesis hash decode: {e}")))?;
    let genesis_hash: [u8; 32] = genesis_hash_bytes
        .as_slice()
        .try_into()
        .map_err(|_| DontYeetWalletError::Chain("genesis hash is not 32 bytes".into()))?;

    let payment = PaymentParams {
        sender,
        receiver,
        micro_algos,
        fee: params.min_fee,
        first_valid: params.last_round,
        last_valid: params.last_round.saturating_add(VALIDITY_ROUNDS),
        genesis_id: params.genesis_id,
        genesis_hash,
    };
    signing::build_signed_payment(&payment, &private_key)
}

/// Broadcast a pre-signed Algorand transaction.
///
/// POSTs the raw msgpack to `/v2/transactions`. Returns the broadcast txid.
///
/// # Errors
/// - [`DontYeetWalletError::Network`] for unknown `chain_id` or HTTP failure.
pub async fn broadcast_signed(chain_id: &str, signed: &[u8]) -> Result<String> {
    if chain_id != "algorand" {
        return Err(DontYeetWalletError::Network(format!(
            "in-browser Algorand broadcaster does not handle {chain_id}"
        )));
    }
    broadcast_raw(signed).await
}

/// Sign and broadcast a native-ALGO Payment entirely client-side.
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

/// Fetch the suggested transaction parameters from Algod.
async fn fetch_tx_params() -> Result<SuggestedParams> {
    let url = format!("{ALGOD_MAINNET}/v2/transactions/params");
    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("params fetch failed: {e}")))?;
    if !resp.ok() {
        return Err(DontYeetWalletError::Network(format!(
            "{url} returned status {}",
            resp.status()
        )));
    }
    resp.json()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("params parse: {e}")))
}

/// Broadcast the raw signed msgpack via `POST /v2/transactions`.
///
/// Algod expects `Content-Type: application/x-binary` with the raw
/// signed-transaction bytes (no envelope, no encoding). The response
/// is JSON: `{ "txId": "..." }`.
async fn broadcast_raw(signed: &[u8]) -> Result<String> {
    let url = format!("{ALGOD_MAINNET}/v2/transactions");

    // Build a `Uint8Array` view over the signed bytes — gloo_net's
    // `body` takes anything `Into<JsValue>`, and `Uint8Array` is the
    // canonical wasm-bindgen handle for raw byte buffers.
    let body: JsValue = Uint8Array::from(signed).into();

    let resp = Request::post(&url)
        .header("Content-Type", "application/x-binary")
        .body(body)
        .map_err(|e| DontYeetWalletError::Network(format!("body build: {e}")))?
        .send()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("broadcast send: {e}")))?;

    if !resp.ok() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        return Err(DontYeetWalletError::Network(format!(
            "broadcast {url} returned status {status}: {body_text}"
        )));
    }

    let body: BroadcastResponse = resp
        .json()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("broadcast response parse: {e}")))?;

    Ok(body.tx_id)
}

// ---------------------------------------------------------------------------
// Response shapes
// ---------------------------------------------------------------------------

/// Subset of the `/v2/accounts/{addr}` response we care about.
#[derive(Deserialize)]
struct AccountInfo {
    /// Balance in microAlgos.
    amount: u64,
}

/// Subset of `/v2/transactions/params` we care about.
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct SuggestedParams {
    genesis_hash: String,
    genesis_id: String,
    last_round: u64,
    min_fee: u64,
}

/// `/v2/transactions` broadcast response.
#[derive(Deserialize)]
struct BroadcastResponse {
    #[serde(rename = "txId")]
    tx_id: String,
}

// Rust guideline compliant 2026-02-21
