//! In-browser TRON balance reads and transaction signing.
//!
//! Two entry points, both browser-only:
//!
//! - [`fetch_balance`] — Phase M.3.2.3. Calls
//!   `POST /wallet/getaccount` directly from the WASM bundle, no
//!   server hop.
//! - [`send`] — Phase M.4.3. End-to-end client-side send pipeline:
//!   derive keypair → derive sender T-address → POST
//!   `/wallet/createtransaction` → sign `raw_data_hex` (via
//!   [`crate::signing`]) → POST `/wallet/broadcasttransaction`.
//!
//! Lives outside the [`feature = "rpc"`] gate because `gloo_net` is
//! browser-only; `#[cfg]`-gated to `wasm32` targets so it never enters
//! native server builds.
//!
//! Mainnet only — testnet (Shasta, Nile) calls fall back to the
//! server-proxied Path A endpoint.

use serde::Deserialize;

use gloo_net::http::Request;

use dontyeet_crypto::{Bip44Deriver, paths};
use dontyeet_primitives::secret::Seed;
use dontyeet_primitives::{Amount, DontYeetWalletError, Result};

use crate::keys::derive_address;
use crate::signing;

/// 1 TRX = `1_000_000` SUN, so balances render with 6 decimal places.
const TRX_DECIMALS: u8 = 6;

/// `TronGrid` mainnet REST base URL.
///
/// Matches the entry in [`config::default_tron_config`](crate::config)
/// — keep the two in sync.
const TRONGRID_MAINNET: &str = "https://api.trongrid.io";

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Fetch the on-chain TRX balance for `address` on TRON mainnet.
///
/// Calls `POST /wallet/getaccount` with the Base58 address
/// (`{"address": ..., "visible": true}`) and reads back the `balance`
/// field, expressed in SUN (1 TRX = 1e6 SUN).
///
/// # Errors
/// - [`DontYeetWalletError::Network`] if `chain_id` isn't `"tron"`, the
///   HTTP call fails (CORS rejection, transport error, non-2xx
///   status), or the body can't be parsed as JSON.
pub async fn fetch_balance(chain_id: &str, address: &str) -> Result<Amount> {
    if chain_id != "tron" {
        return Err(DontYeetWalletError::Network(format!(
            "in-browser TRON fetcher does not handle {chain_id}"
        )));
    }

    let url = format!("{TRONGRID_MAINNET}/wallet/getaccount");
    let body = serde_json::json!({
        "address": address,
        "visible": true,
    });

    let request = Request::post(&url)
        .json(&body)
        .map_err(|e| DontYeetWalletError::Network(format!("request build failed: {e}")))?;

    let resp = request
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

    Ok(Amount::from_raw(u128::from(info.balance), TRX_DECIMALS))
}

/// Build a signed TRON transaction envelope without broadcasting.
///
/// Pipeline: derive keypair → POST `createtransaction` for the
/// unsigned envelope → sign + splice. Returns the JSON-serialized
/// envelope bytes ready for `broadcasttransaction`.
///
/// # Errors
/// Same shape as the previous `send`, minus the broadcast step.
pub async fn build_signed_payload(
    chain_id: &str,
    seed: &Seed,
    to: &str,
    amount_raw: &str,
) -> Result<Vec<u8>> {
    if chain_id != "tron" {
        return Err(DontYeetWalletError::Network(format!(
            "in-browser TRON sender does not handle {chain_id}"
        )));
    }

    let private_key = Bip44Deriver::derive(seed, paths::TRON)?;
    let sender = derive_address(seed, paths::TRON)?;

    let amount_sun: u64 = amount_raw
        .parse()
        .map_err(|e| DontYeetWalletError::Validation(format!("invalid amount '{amount_raw}': {e}")))?;

    let create_url = format!("{TRONGRID_MAINNET}/wallet/createtransaction");
    let create_body = serde_json::json!({
        "owner_address": sender.as_str(),
        "to_address": to,
        "amount": amount_sun,
        "visible": true,
    });

    let mut tx_json = post_json(&create_url, &create_body).await?;

    let raw_data_hex = tx_json
        .get("raw_data_hex")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            DontYeetWalletError::Chain("missing raw_data_hex in createtransaction response".into())
        })?
        .to_owned();
    signing::sign_and_attach(&mut tx_json, &raw_data_hex, &private_key)?;

    serde_json::to_vec(&tx_json)
        .map_err(|e| DontYeetWalletError::Chain(format!("envelope serialize: {e}")))
}

/// Broadcast a pre-signed TRON envelope.
///
/// `signed` is the JSON-serialized envelope produced by
/// [`build_signed_payload`]. POSTs to `/wallet/broadcasttransaction`.
///
/// # Errors
/// - [`DontYeetWalletError::Network`] for unknown `chain_id` or HTTP failure.
/// - [`DontYeetWalletError::Chain`] if the envelope can't be re-parsed or
///   the broadcast indicates failure.
pub async fn broadcast_signed(chain_id: &str, signed: &[u8]) -> Result<String> {
    if chain_id != "tron" {
        return Err(DontYeetWalletError::Network(format!(
            "in-browser TRON broadcaster does not handle {chain_id}"
        )));
    }

    let tx_json: serde_json::Value = serde_json::from_slice(signed)
        .map_err(|e| DontYeetWalletError::Chain(format!("envelope re-parse: {e}")))?;

    let broadcast_url = format!("{TRONGRID_MAINNET}/wallet/broadcasttransaction");
    let broadcast_resp: BroadcastResponse =
        serde_json::from_value(post_json(&broadcast_url, &tx_json).await?)
            .map_err(|e| DontYeetWalletError::Network(format!("broadcast response parse: {e}")))?;

    if !broadcast_resp.result && broadcast_resp.txid.is_empty() {
        return Err(DontYeetWalletError::Chain(format!(
            "TRON broadcast failed: {}",
            broadcast_resp
                .message
                .unwrap_or_else(|| "no txid returned".into())
        )));
    }

    Ok(broadcast_resp.txid)
}

/// Sign and broadcast a native TRX transfer entirely client-side.
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

/// Send a JSON POST and return the parsed response body.
///
/// Centralizes the `gloo_net` error-mapping shape used by both the
/// `createtransaction` and `broadcasttransaction` calls.
async fn post_json(url: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
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

// ---------------------------------------------------------------------------
// Response shapes
// ---------------------------------------------------------------------------

/// Subset of the `/wallet/getaccount` response we care about.
#[derive(Deserialize)]
struct AccountInfo {
    /// Balance in SUN. Absent for new/unfunded accounts — the
    /// `serde(default)` makes that case decode as `0`, matching the
    /// server-side fetcher's behavior.
    #[serde(default)]
    balance: u64,
}

/// Subset of the `/wallet/broadcasttransaction` response.
#[derive(Deserialize)]
struct BroadcastResponse {
    #[serde(default)]
    txid: String,
    #[serde(default)]
    result: bool,
    /// Optional error message — `TronGrid` returns this hex-encoded on
    /// failure; we surface it as-is in the error message.
    #[serde(default)]
    message: Option<String>,
}

// Rust guideline compliant 2026-02-21
