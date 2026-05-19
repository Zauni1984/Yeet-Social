//! In-browser XRP Ledger balance reads and transaction signing.
//!
//! Two entry points, both browser-only:
//!
//! - [`fetch_balance`] — Phase M.3.2.4. Calls the `account_info`
//!   JSON-RPC method directly from the WASM bundle, no server hop.
//! - [`send`] — Phase M.4.4. End-to-end client-side send pipeline:
//!   derive keypair → derive sender's `r...` address → fetch
//!   account sequence + current ledger index → call
//!   [`crate::signing::build_signed_payment`] → broadcast via the
//!   `submit` JSON-RPC method.
//!
//! Lives outside the [`feature = "rpc"`] gate; `#[cfg]`-gated to
//! `wasm32` targets so server builds stay unaffected.
//!
//! Mainnet only — testnet calls fall back to the server-proxied
//! Path A endpoint.

use serde::Deserialize;

use gloo_net::http::Request;

use dontyeet_crypto::{Bip44Deriver, paths};
use dontyeet_primitives::secret::Seed;
use dontyeet_primitives::{Amount, DontYeetWalletError, Result};

use crate::keys::{compressed_pubkey, derive_address};
use crate::signing::{self, PaymentParams};

/// 1 XRP = `1_000_000` drops, so balances render with 6 decimal places.
const XRP_DECIMALS: u8 = 6;

/// XRP Ledger mainnet JSON-RPC endpoint.
///
/// Matches the entry in [`config::default_xrp_config`](crate::config)
/// — keep in sync.
const XRP_MAINNET_RPC: &str = "https://s1.ripple.com:51234/";

/// Ledger validity window: ~80 seconds at ~4 s / ledger. Mirrors the
/// server-side `transfer::LEDGER_OFFSET`.
const LEDGER_OFFSET: u64 = 20;

/// XRPL base fee in drops on mainnet. Mirrors
/// `transfer::BASE_FEE_DROPS`.
const BASE_FEE_DROPS: u64 = 12;

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Fetch the on-chain XRP balance for `address` on mainnet.
///
/// Calls the `account_info` JSON-RPC method against the public
/// Ripple node and reads the `Balance` field, expressed in drops
/// (1 XRP = 1e6 drops). New / unfunded accounts surface as
/// [`DontYeetWalletError::NotFound`] — the same shape the server-side
/// fetcher uses, so the UI's existing handling carries over.
///
/// # Errors
/// - [`DontYeetWalletError::Network`] if `chain_id` isn't `"xrp"`, the
///   HTTP call fails, or the body can't be parsed.
/// - [`DontYeetWalletError::NotFound`] if the account isn't on the ledger
///   yet (a fresh wallet that has never received XRP).
/// - [`DontYeetWalletError::Chain`] if the `Balance` field can't be parsed
///   as a `u64`.
pub async fn fetch_balance(chain_id: &str, address: &str) -> Result<Amount> {
    if chain_id != "xrp" {
        return Err(DontYeetWalletError::Network(format!(
            "in-browser XRP fetcher does not handle {chain_id}"
        )));
    }

    let body = serde_json::json!({
        "method": "account_info",
        "params": [{
            "account": address,
            "ledger_index": "validated"
        }]
    });

    let info: AccountInfoResponse = post_json(XRP_MAINNET_RPC, &body).await?;

    if let Some(err) = info.result.error {
        return Err(DontYeetWalletError::NotFound(format!(
            "XRP account_info error: {err}"
        )));
    }

    let account_data = info
        .result
        .account_data
        .ok_or_else(|| DontYeetWalletError::NotFound("XRP account data missing from response".into()))?;

    let drops: u64 = account_data
        .balance
        .parse()
        .map_err(|e| DontYeetWalletError::Chain(format!("balance parse error: {e}")))?;

    Ok(Amount::from_raw(u128::from(drops), XRP_DECIMALS))
}

/// Build a signed XRP Payment without broadcasting.
///
/// Returns the raw signed payload bytes ready for hex-encoding into
/// `submit`.
///
/// # Errors
/// Same shape as the previous `send`, minus the broadcast step.
pub async fn build_signed_payload(
    chain_id: &str,
    seed: &Seed,
    to: &str,
    amount_raw: &str,
) -> Result<Vec<u8>> {
    if chain_id != "xrp" {
        return Err(DontYeetWalletError::Network(format!(
            "in-browser XRP sender does not handle {chain_id}"
        )));
    }

    let private_key = Bip44Deriver::derive(seed, paths::XRP)?;
    let signing_pub_key = compressed_pubkey(&private_key)?;
    let sender = derive_address(seed, paths::XRP)?;
    let from_account_id = signing::address_to_account_id(sender.as_str())?;
    let to_account_id = signing::address_to_account_id(to)?;

    let drops: u64 = amount_raw
        .parse()
        .map_err(|e| DontYeetWalletError::Validation(format!("invalid amount '{amount_raw}': {e}")))?;

    let (sequence, ledger_index) = fetch_account_state(sender.as_str()).await?;
    let last_ledger_seq = ledger_index.saturating_add(LEDGER_OFFSET);

    let params = PaymentParams {
        from_account_id,
        to_account_id,
        drops,
        fee_drops: BASE_FEE_DROPS,
        sequence,
        last_ledger_seq,
        signing_pub_key,
    };
    signing::build_signed_payment(&params, &private_key)
}

/// Broadcast a pre-signed XRP transaction.
///
/// Hex-encodes the bytes (uppercase) and submits via `submit`. Returns
/// the broadcast tx hash.
///
/// # Errors
/// - [`DontYeetWalletError::Network`] for unknown `chain_id`, RPC failure,
///   or a `submit` error response.
pub async fn broadcast_signed(chain_id: &str, signed: &[u8]) -> Result<String> {
    if chain_id != "xrp" {
        return Err(DontYeetWalletError::Network(format!(
            "in-browser XRP broadcaster does not handle {chain_id}"
        )));
    }
    let tx_blob = hex::encode(signed).to_uppercase();
    let body = serde_json::json!({
        "method": "submit",
        "params": [{ "tx_blob": tx_blob }]
    });
    let submit: SubmitResponse = post_json(XRP_MAINNET_RPC, &body).await?;

    if let Some(err) = submit.result.error {
        return Err(DontYeetWalletError::Network(format!("XRP submit error: {err}")));
    }

    let tx_json = submit
        .result
        .tx_json
        .ok_or_else(|| DontYeetWalletError::Network("XRP submit response missing tx_json".into()))?;

    Ok(tx_json.hash)
}

/// Sign and broadcast a native-XRP Payment entirely client-side.
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
/// Centralizes the `gloo_net` error-mapping shape used by every XRPL
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

/// Fetch account sequence + current ledger index via JSON-RPC.
async fn fetch_account_state(account: &str) -> Result<(u64, u64)> {
    let acct_body = serde_json::json!({
        "method": "account_info",
        "params": [{ "account": account }]
    });
    let acct: AccountInfoResponse = post_json(XRP_MAINNET_RPC, &acct_body).await?;
    if let Some(err) = acct.result.error {
        return Err(DontYeetWalletError::NotFound(format!(
            "account_info error: {err}"
        )));
    }
    let sequence = acct
        .result
        .account_data
        .ok_or_else(|| DontYeetWalletError::NotFound("account_info: missing account_data".into()))?
        .sequence;

    let ledger_body = serde_json::json!({
        "method": "ledger_current",
        "params": [{}]
    });
    let ledger: LedgerCurrentResponse = post_json(XRP_MAINNET_RPC, &ledger_body).await?;

    Ok((sequence, ledger.result.ledger_current_index))
}

// ---------------------------------------------------------------------------
// Response shapes
// ---------------------------------------------------------------------------

/// Outer JSON-RPC envelope for `account_info`.
#[derive(Deserialize)]
struct AccountInfoResponse {
    result: AccountInfoResult,
}

/// Inner `account_info` result.
#[derive(Deserialize)]
struct AccountInfoResult {
    account_data: Option<AccountData>,
    error: Option<String>,
}

/// Subset of XRPL account data we care about.
///
/// On-the-wire field names are `Balance` and `Sequence`; we
/// lower-case via `serde(rename)` so the Rust fields stay idiomatic.
#[derive(Deserialize)]
struct AccountData {
    #[serde(rename = "Balance")]
    balance: String,
    #[serde(rename = "Sequence")]
    sequence: u64,
}

/// Outer JSON-RPC envelope for `ledger_current`.
#[derive(Deserialize)]
struct LedgerCurrentResponse {
    result: LedgerCurrentResult,
}

/// Inner `ledger_current` result.
#[derive(Deserialize)]
struct LedgerCurrentResult {
    ledger_current_index: u64,
}

/// Outer JSON-RPC envelope for `submit`.
#[derive(Deserialize)]
struct SubmitResponse {
    result: SubmitResult,
}

/// Inner `submit` result.
#[derive(Deserialize)]
struct SubmitResult {
    tx_json: Option<SubmitTxJson>,
    error: Option<String>,
}

/// Subset of the `tx_json` field returned by `submit`.
#[derive(Deserialize)]
struct SubmitTxJson {
    hash: String,
}

// Rust guideline compliant 2026-02-21
