//! In-browser Kadena balance reads and transaction signing.
//!
//! Two entry points, both browser-only:
//!
//! - [`fetch_balance`] — Phase M.3.2.8. POSTs a Pact `local` command
//!   running `(coin.get-balance "<address>")` against community-mainnet
//!   chain 0, no server hop.
//! - [`send`] — Phase M.4.8. End-to-end client-side send pipeline:
//!   derive keypair → derive sender's `k:` address → stamp current
//!   Unix time via `js_sys::Date::now()` → build + sign via
//!   [`crate::signing::build_signed_transfer`] → broadcast via
//!   `POST {base}/chainweb/0.0/mainnet01/chain/0/pact/api/v1/send`.
//!
//! The browser path defaults to **community mainnet** — the
//! post-fork (Nov 2025) Chainweb that this wallet treats as the
//! canonical Kadena. The legacy chain (`api.chainweb.com`,
//! abandoned but still reachable) and community testnet remain
//! supported through the unchanged `kadena_plugin` config and the
//! server-proxied Path A fallback; per-network browser routing is
//! a follow-up phase.
//!
//! Lives outside the [`feature = "rpc"`] gate; `#[cfg]`-gated to
//! `wasm32` targets so server builds stay unaffected.

use serde::Deserialize;

use gloo_net::http::Request;

use dontyeet_crypto::{Bip44Deriver, paths};
use dontyeet_primitives::secret::Seed;
use dontyeet_primitives::{Address, Amount, DontYeetWalletError, Result};

use crate::keys::derive_address;
use crate::signing;

/// KDA's Pact `coin` contract uses 12 decimal places.
const KDA_DECIMALS: u8 = 12;

/// Default Chainweb chain to query / broadcast against.
///
/// Kadena replicates account state across all 20 parallel chains;
/// chain 0 is the conventional pick for wallet display.
const DEFAULT_CHAIN: &str = "0";

/// Chainweb network version for the community-mainnet endpoint.
///
/// Pinned to the post-fork community Chainweb that the wallet
/// targets — see [`crate::plugin::kadena_plugin`] for the matching
/// server config.
const NETWORK_VERSION: &str = "mainnet01";

/// Community Chainweb mainnet REST API base.
///
/// Matches the entry in [`crate::plugin::kadena_plugin`] — keep in
/// sync.
const CHAINWEB_MAINNET: &str = "https://api.chainweb-community.org";

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Fetch the on-chain KDA balance for `address` on community mainnet.
///
/// POSTs a Pact `local` command running `(coin.get-balance "<address>")`
/// against chain 0, then parses the returned decimal into a fixed
/// 12-decimal-place [`Amount`].
///
/// # Errors
/// - [`DontYeetWalletError::Network`] if `chain_id` isn't `"kadena"`, the
///   HTTP call fails (CORS rejection, transport error, non-2xx
///   status), or the response can't be parsed.
/// - [`DontYeetWalletError::Chain`] if the returned decimal can't be
///   converted to a `u128` raw amount (overflow / malformed digits).
pub async fn fetch_balance(chain_id: &str, address: &str) -> Result<Amount> {
    if chain_id != "kadena" {
        return Err(DontYeetWalletError::Network(format!(
            "in-browser Kadena fetcher does not handle {chain_id}"
        )));
    }

    let url = format!(
        "{CHAINWEB_MAINNET}/chainweb/0.0/{NETWORK_VERSION}/chain/{DEFAULT_CHAIN}/pact/api/v1/local"
    );

    let cmd_inner = serde_json::json!({
        "networkId": NETWORK_VERSION,
        "payload": {
            "exec": {
                "data": {},
                "code": format!("(coin.get-balance \"{address}\")"),
            }
        },
        "signers": [],
        "meta": {
            "chainId": DEFAULT_CHAIN,
            "sender": "",
            "gasLimit": 1000,
            "gasPrice": 1e-8_f64,
            "ttl": 600,
            "creationTime": 0,
        },
        "nonce": "balance-query"
    })
    .to_string();

    let body = serde_json::json!({
        "cmd": cmd_inner,
        "hash": "",
        "sigs": [],
    });

    let request = Request::post(&url)
        .json(&body)
        .map_err(|e| DontYeetWalletError::Network(format!("request build failed: {e}")))?;

    let resp = request
        .send()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("Pact local fetch failed: {e}")))?;

    if !resp.ok() {
        return Err(DontYeetWalletError::Network(format!(
            "{url} returned status {}",
            resp.status()
        )));
    }

    let parsed: PactLocalResponse = resp
        .json()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("Pact local parse: {e}")))?;

    // A failed Pact execution (e.g. account not yet created on this
    // chain) is reported as a `failure` status; surface that as a
    // zero balance to match the server-side fetcher.
    if parsed.result.status != "success" {
        return Ok(Amount::from_raw(0, KDA_DECIMALS));
    }

    parse_kda_balance(&parsed.result.data)
}

/// Build a signed Kadena `coin.transfer` envelope without broadcasting.
///
/// Returns the JSON-serialized envelope bytes ready for
/// `/pact/api/v1/send`.
///
/// Unlike other chains, this build phase performs no I/O — Kadena's
/// Pact `coin.transfer` only needs a wall-clock stamp (filled below)
/// and the user's keypair, both available offline. The function still
/// returns a `Future` to keep the per-chain interface uniform.
///
/// # Errors
/// Same shape as the previous `send`, minus the broadcast step.
#[expect(
    clippy::unused_async,
    reason = "uniform interface with other chains' build_signed_payload; \
              future versions may fetch fee data and become genuinely async"
)]
pub async fn build_signed_payload(
    chain_id: &str,
    seed: &Seed,
    to: &str,
    amount_raw: &str,
) -> Result<Vec<u8>> {
    if chain_id != "kadena" {
        return Err(DontYeetWalletError::Network(format!(
            "in-browser Kadena sender does not handle {chain_id}"
        )));
    }

    let private_key = Bip44Deriver::derive(seed, paths::KADENA)?;
    let sender = derive_address(seed, paths::KADENA)?;

    let raw: u128 = amount_raw
        .parse()
        .map_err(|e| DontYeetWalletError::Validation(format!("invalid amount '{amount_raw}': {e}")))?;
    let amount = Amount::from_raw(raw, KDA_DECIMALS);

    // Stamp current Unix-epoch seconds via JS Date.
    //
    // `Date::now()` returns ms since epoch as f64 (always positive,
    // always integral within JS's safe-integer range). Truncation to
    // u64 is safe — the value will not exceed u64::MAX for billions
    // of years.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "Date::now() is a positive integral millisecond count; \
                  /1000 then truncate-to-u64 is the canonical Pact creationTime form"
    )]
    let creation_time_secs = (js_sys::Date::now() / 1000.0) as u64;

    let recipient = Address::new(to.to_owned());
    signing::build_signed_transfer(
        NETWORK_VERSION,
        &sender,
        &recipient,
        &amount,
        &private_key,
        creation_time_secs,
    )
}

/// Broadcast a pre-signed Kadena envelope.
///
/// `signed` is the JSON-serialized envelope. POSTs `{"cmds": [<env>]}`
/// to `/pact/api/v1/send` and returns the first request key.
///
/// # Errors
/// - [`DontYeetWalletError::Network`] for unknown `chain_id` or HTTP failure.
/// - [`DontYeetWalletError::Chain`] if the envelope can't be re-parsed or
///   the response is missing the request key.
pub async fn broadcast_signed(chain_id: &str, signed: &[u8]) -> Result<String> {
    if chain_id != "kadena" {
        return Err(DontYeetWalletError::Network(format!(
            "in-browser Kadena broadcaster does not handle {chain_id}"
        )));
    }

    let envelope: serde_json::Value = serde_json::from_slice(signed)
        .map_err(|e| DontYeetWalletError::Chain(format!("envelope re-parse: {e}")))?;
    let body = serde_json::json!({ "cmds": [envelope] });

    let url = format!(
        "{CHAINWEB_MAINNET}/chainweb/0.0/{NETWORK_VERSION}/chain/{DEFAULT_CHAIN}/pact/api/v1/send"
    );
    let resp = Request::post(&url)
        .json(&body)
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

    let parsed: SendResponse = resp
        .json()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("send response parse: {e}")))?;

    parsed
        .request_keys
        .into_iter()
        .next()
        .ok_or_else(|| DontYeetWalletError::Chain("no request key in send response".into()))
}

/// Sign and broadcast a native KDA `coin.transfer` entirely client-side.
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
// Internal helpers
// ---------------------------------------------------------------------------

/// Parse a Pact decimal balance into a 12-decimal-place [`Amount`].
///
/// Pact returns balances either as a JSON `Number`, a `String`, or
/// an object `{"decimal": "..."}`. Mirrors
/// [`balance::parse_kda_balance`](crate::balance) — duplicated here
/// because the server-side function lives behind `feature = "rpc"`.
fn parse_kda_balance(data: &serde_json::Value) -> Result<Amount> {
    let decimal_str = match data {
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Object(obj) => obj
            .get("decimal")
            .and_then(|v| v.as_str())
            .unwrap_or("0")
            .to_string(),
        serde_json::Value::String(s) => s.clone(),
        _ => return Ok(Amount::from_raw(0, KDA_DECIMALS)),
    };

    let parts: Vec<&str> = decimal_str.split('.').collect();
    let integer_part: u128 = parts[0]
        .parse()
        .map_err(|e| DontYeetWalletError::Chain(format!("invalid balance integer: {e}")))?;

    let fractional_raw = if parts.len() > 1 {
        let frac = parts[1];
        let padded = format!("{frac:0<width$}", width = KDA_DECIMALS as usize);
        let truncated = &padded[..KDA_DECIMALS as usize];
        truncated
            .parse::<u128>()
            .map_err(|e| DontYeetWalletError::Chain(format!("invalid balance fraction: {e}")))?
    } else {
        0
    };

    let multiplier: u128 = 10_u128.pow(u32::from(KDA_DECIMALS));
    let raw = integer_part
        .checked_mul(multiplier)
        .and_then(|v| v.checked_add(fractional_raw))
        .ok_or_else(|| DontYeetWalletError::Chain("balance arithmetic overflow".into()))?;

    Ok(Amount::from_raw(raw, KDA_DECIMALS))
}

// ---------------------------------------------------------------------------
// Response shapes
// ---------------------------------------------------------------------------

/// JSON response from the Pact `/local` endpoint.
#[derive(Deserialize)]
struct PactLocalResponse {
    result: PactResult,
}

/// The `result` field inside a Pact local response.
#[derive(Deserialize)]
struct PactResult {
    /// `"success"` or `"failure"`.
    status: String,
    /// Returned data — for `coin.get-balance` this is a decimal value
    /// (number, string, or `{"decimal": "..."}` object).
    data: serde_json::Value,
}

/// JSON response from `POST .../pact/api/v1/send`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendResponse {
    /// One request key per submitted command (we send exactly one).
    request_keys: Vec<String>,
}

// Rust guideline compliant 2026-02-21
