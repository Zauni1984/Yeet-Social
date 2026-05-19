//! In-browser Cardano balance reads and transaction signing.
//!
//! Two entry points, both browser-only:
//!
//! - [`fetch_balance`] — Phase M.3.2.10. POSTs to Koios's
//!   `address_info` endpoint, no server hop.
//! - [`send`] — Phase M.4.9. End-to-end client-side send pipeline:
//!   derive keypair → derive sender's `addr1...` address → fetch
//!   UTXOs + tip + epoch params from Koios → call
//!   [`crate::signing::build_signed_transfer`] → broadcast via Koios
//!   `submittx` with `Content-Type: application/cbor`.
//!
//! Lives outside the [`feature = "rpc"`] gate; `#[cfg]`-gated to
//! `wasm32` targets so server builds stay unaffected.
//!
//! ## Why Koios (browser) and not Blockfrost (server)?
//!
//! Blockfrost requires a project-id header that the browser bundle
//! can't safely carry. [Koios](https://api.koios.rest) is a
//! community-run, key-less Cardano REST gateway with the same data,
//! plus CORS support — exactly what direct browser reads and writes
//! need. The server-side flow keeps using Blockfrost (paid tier
//! gives better SLAs) and lives in [`crate::transfer`].
//!
//! Mainnet only — preprod / preview reads stay on Path A.

use serde::Deserialize;

use gloo_net::http::Request;
use js_sys::Uint8Array;
use wasm_bindgen::JsValue;

use dontyeet_crypto::{Bip44Deriver, paths};
use dontyeet_primitives::secret::Seed;
use dontyeet_primitives::{Amount, DontYeetWalletError, Result};

use crate::keys::{derive_address, ed25519_pubkey};
use crate::signing::{self, Utxo};

/// 1 ADA = `1_000_000` lovelace — 6 decimal places.
const ADA_DECIMALS: u8 = 6;

/// Koios mainnet REST API base.
///
/// All calls below append a path to this base. Pinned to v1 of the
/// Koios API.
const KOIOS_BASE: &str = "https://api.koios.rest/api/v1";

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Fetch the on-chain ADA balance for `address` on mainnet.
///
/// POSTs `{"_addresses": ["<address>"]}` to Koios's
/// [`address_info`](https://api.koios.rest/#post-/address_info)
/// endpoint and reads the `balance` field, expressed in lovelace
/// (1 ADA = 1e6 lovelace). New / unfunded addresses come back as
/// an empty array, which decodes as a zero balance — same shape
/// the server-side Blockfrost fetcher exposes.
///
/// # Errors
/// - [`DontYeetWalletError::Network`] if `chain_id` isn't `"cardano"`,
///   the HTTP call fails (CORS rejection, transport error, non-2xx
///   status), or the response can't be parsed.
/// - [`DontYeetWalletError::Chain`] if the `balance` string can't be
///   parsed as a `u128` lovelace amount.
pub async fn fetch_balance(chain_id: &str, address: &str) -> Result<Amount> {
    if chain_id != "cardano" {
        return Err(DontYeetWalletError::Network(format!(
            "in-browser Cardano fetcher does not handle {chain_id}"
        )));
    }

    let url = format!("{KOIOS_BASE}/address_info");
    let body = serde_json::json!({ "_addresses": [address] });

    let entries: Vec<AddressInfo> = post_json(&url, &body).await?;

    let Some(entry) = entries.into_iter().next() else {
        return Ok(Amount::from_raw(0, ADA_DECIMALS));
    };

    let lovelace: u128 = entry
        .balance
        .parse()
        .map_err(|e| DontYeetWalletError::Chain(format!("lovelace parse error: {e}")))?;

    Ok(Amount::from_raw(lovelace, ADA_DECIMALS))
}

/// Build a signed Cardano payment transaction without broadcasting.
///
/// Returns the raw signed CBOR bytes ready for `/submittx`.
///
/// # Errors
/// Same shape as the previous `send`, minus the broadcast step.
pub async fn build_signed_payload(
    chain_id: &str,
    seed: &Seed,
    to: &str,
    amount_raw: &str,
) -> Result<Vec<u8>> {
    if chain_id != "cardano" {
        return Err(DontYeetWalletError::Network(format!(
            "in-browser Cardano sender does not handle {chain_id}"
        )));
    }

    let private_key = Bip44Deriver::derive(seed, paths::CARDANO)?;
    let sender_pubkey = ed25519_pubkey(&private_key)?;
    let sender_addr = derive_address(seed, paths::CARDANO)?;
    let from_bytes = signing::decode_bech32_address(sender_addr.as_str())?;
    let to_bytes = signing::decode_bech32_address(to)?;

    let lovelace: u64 = amount_raw
        .parse()
        .map_err(|e| DontYeetWalletError::Validation(format!("invalid amount '{amount_raw}': {e}")))?;

    let utxos = fetch_utxos(sender_addr.as_str()).await?;
    let tip = fetch_tip().await?;
    let params = fetch_epoch_params(tip.epoch_no).await?;

    signing::build_signed_transfer(
        &utxos,
        params.min_fee_a,
        params.min_fee_b,
        tip.abs_slot,
        &from_bytes,
        &to_bytes,
        &sender_pubkey,
        lovelace,
        &private_key,
    )
}

/// Broadcast a pre-signed Cardano transaction.
///
/// POSTs the raw CBOR bytes to Koios `/submittx`. Returns the tx hash.
///
/// # Errors
/// - [`DontYeetWalletError::Network`] for unknown `chain_id` or HTTP failure.
pub async fn broadcast_signed(chain_id: &str, signed: &[u8]) -> Result<String> {
    if chain_id != "cardano" {
        return Err(DontYeetWalletError::Network(format!(
            "in-browser Cardano broadcaster does not handle {chain_id}"
        )));
    }
    submit_tx(signed).await
}

/// Sign and broadcast a native ADA payment entirely client-side.
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
// HTTP plumbing — Koios JSON POSTs
// ---------------------------------------------------------------------------

/// POST a JSON-RPC body and decode the response into `T`.
///
/// Koios uses POST for nearly every read endpoint; the body is a
/// JSON object describing the query. Centralizes the `gloo_net`
/// error-mapping shape used by the M.3.2.10 balance fetch and every
/// M.4.9 send-pipeline call.
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

/// Fetch UTXOs for `address` and convert to [`signing::Utxo`].
async fn fetch_utxos(address: &str) -> Result<Vec<Utxo>> {
    let url = format!("{KOIOS_BASE}/address_utxos");
    let body = serde_json::json!({
        "_addresses": [address],
        "_extended": false,
    });
    let raw: Vec<KoiosUtxo> = post_json(&url, &body).await?;

    Ok(raw
        .into_iter()
        .filter_map(|u| {
            let lovelace = u.value.parse::<u64>().ok()?;
            Some(Utxo {
                tx_hash: u.tx_hash,
                output_index: u.tx_index,
                lovelace,
            })
        })
        .collect())
}

/// Fetch the latest tip (current epoch + absolute slot).
async fn fetch_tip() -> Result<TipInfo> {
    let url = format!("{KOIOS_BASE}/tip");
    // Koios `/tip` returns a single-element array; we take the first.
    let entries: Vec<TipInfo> = post_json(&url, &serde_json::json!({})).await?;
    entries
        .into_iter()
        .next()
        .ok_or_else(|| DontYeetWalletError::Chain("Koios /tip returned empty array".into()))
}

/// Fetch protocol params for the given epoch.
async fn fetch_epoch_params(epoch_no: u64) -> Result<ProtocolParams> {
    let url = format!("{KOIOS_BASE}/epoch_params");
    let body = serde_json::json!({ "_epoch_no": epoch_no });
    let entries: Vec<ProtocolParams> = post_json(&url, &body).await?;
    entries
        .into_iter()
        .next()
        .ok_or_else(|| DontYeetWalletError::Chain("Koios /epoch_params returned empty array".into()))
}

/// Submit a signed CBOR transaction via Koios `/submittx`.
///
/// `submittx` expects `Content-Type: application/cbor` with the raw
/// signed-transaction bytes and returns the transaction hash as a
/// JSON string on success.
async fn submit_tx(signed: &[u8]) -> Result<String> {
    let url = format!("{KOIOS_BASE}/submittx");

    // Build a `Uint8Array` view over the signed bytes — gloo_net's
    // `body` takes anything `Into<JsValue>`, and `Uint8Array` is the
    // canonical wasm-bindgen handle for raw byte buffers.
    let body: JsValue = Uint8Array::from(signed).into();

    let resp = Request::post(&url)
        .header("Content-Type", "application/cbor")
        .body(body)
        .map_err(|e| DontYeetWalletError::Network(format!("body build: {e}")))?
        .send()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("submittx send: {e}")))?;

    if !resp.ok() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        return Err(DontYeetWalletError::Network(format!(
            "submittx {url} returned status {status}: {body_text}"
        )));
    }

    // Koios returns the tx hash as a JSON string (e.g. `"abcd1234..."`).
    let tx_hash: String = resp
        .json()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("submittx response parse: {e}")))?;

    Ok(tx_hash)
}

// ---------------------------------------------------------------------------
// Response shapes
// ---------------------------------------------------------------------------

/// Subset of the Koios `address_info` response we care about.
#[derive(Deserialize)]
struct AddressInfo {
    /// Total balance at this address, in lovelace, encoded as a
    /// decimal string (Koios serializes large integers as strings
    /// to avoid JS-number precision loss).
    balance: String,
}

/// Subset of one element of `address_utxos` response.
#[derive(Deserialize)]
struct KoiosUtxo {
    tx_hash: String,
    tx_index: u32,
    /// Value in lovelace, decimal-string-encoded.
    value: String,
}

/// Subset of the `/tip` response.
#[derive(Deserialize)]
struct TipInfo {
    epoch_no: u64,
    abs_slot: u64,
}

/// Subset of the `/epoch_params` response.
#[derive(Deserialize)]
struct ProtocolParams {
    min_fee_a: u64,
    min_fee_b: u64,
}

// Rust guideline compliant 2026-02-21
