//! In-browser Kaspa balance reads and transaction signing.
//!
//! Two entry points, both browser-only:
//!
//! - [`fetch_balance`] — Phase M.3.2.7. Calls
//!   `GET {base}/addresses/{address}/balance` directly from the WASM
//!   bundle, no server hop.
//! - [`send`] — Phase M.4.7. End-to-end client-side send pipeline:
//!   derive keypair → derive sender address → fetch UTXOs via
//!   `GET /addresses/{addr}/utxos` → call
//!   [`crate::signing::build_signed_transfer`] → broadcast via
//!   `POST /transactions`.
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

use crate::keys::derive_address;
use crate::signing::{self, Utxo};

/// 1 KAS = `100_000_000` SOMPI — 8 decimal places.
const KAS_DECIMALS: u8 = 8;

/// Public Kaspa mainnet REST API base.
///
/// Matches the entry in [`config::default_kaspa_config`](crate::config)
/// — keep in sync.
const KASPA_MAINNET: &str = "https://api.kaspa.org";

/// Default fee in sompi for a simple transfer. Mirrors
/// `transfer::DEFAULT_FEE_SOMPI`.
const DEFAULT_FEE_SOMPI: u64 = 3000;

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Fetch the on-chain KAS balance for `address` on mainnet.
///
/// Calls `GET {base}/addresses/{address}/balance` and reads the
/// `balance` field, expressed in SOMPI (1 KAS = 1e8 SOMPI). The
/// `kaspa:` prefix in addresses contains a `:` which is URL-reserved;
/// it gets percent-encoded inline before the request goes out.
///
/// # Errors
/// - [`DontYeetWalletError::Network`] if `chain_id` isn't `"kaspa"`, the
///   HTTP call fails (CORS rejection, transport error, non-2xx
///   status), or the body can't be parsed.
pub async fn fetch_balance(chain_id: &str, address: &str) -> Result<Amount> {
    if chain_id != "kaspa" {
        return Err(DontYeetWalletError::Network(format!(
            "in-browser Kaspa fetcher does not handle {chain_id}"
        )));
    }

    let encoded_address = address.replace(':', "%3A");
    let url = format!("{KASPA_MAINNET}/addresses/{encoded_address}/balance");

    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("balance fetch failed: {e}")))?;

    if !resp.ok() {
        return Err(DontYeetWalletError::Network(format!(
            "{url} returned status {}",
            resp.status()
        )));
    }

    let body: BalanceResponse = resp
        .json()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("balance parse: {e}")))?;

    Ok(Amount::from_raw(u128::from(body.balance), KAS_DECIMALS))
}

/// Build a signed Kaspa transfer transaction without broadcasting.
///
/// Returns the JSON-serialized transaction bytes ready for
/// `POST /transactions`.
///
/// # Errors
/// Same shape as the previous `send`, minus the broadcast step.
pub async fn build_signed_payload(
    chain_id: &str,
    seed: &Seed,
    to: &str,
    amount_raw: &str,
) -> Result<Vec<u8>> {
    if chain_id != "kaspa" {
        return Err(DontYeetWalletError::Network(format!(
            "in-browser Kaspa sender does not handle {chain_id}"
        )));
    }

    let private_key = Bip44Deriver::derive(seed, paths::KASPA)?;
    let sender = derive_address(seed, paths::KASPA)?;
    let sender_str = sender.as_str().to_owned();

    let sompi: u64 = amount_raw
        .parse()
        .map_err(|e| DontYeetWalletError::Validation(format!("invalid amount '{amount_raw}': {e}")))?;

    let raw_utxos = fetch_utxos(&sender_str).await?;
    let utxos: Vec<Utxo> = raw_utxos
        .into_iter()
        .filter_map(|u| {
            let value = u.utxo_entry.amount.parse::<u64>().ok()?;
            Some(Utxo {
                txid: u.outpoint.transaction_id,
                index: u.outpoint.index,
                value_sompi: value,
                script_hex: u.utxo_entry.script_public_key.script_public_key,
            })
        })
        .collect();

    signing::build_signed_transfer(
        &utxos,
        sompi,
        DEFAULT_FEE_SOMPI,
        &sender_str,
        to,
        &private_key,
    )
}

/// Broadcast a pre-signed Kaspa transaction.
///
/// `signed` is the JSON-serialized transaction. POSTs to
/// `/transactions`. Returns the broadcast transaction id.
///
/// # Errors
/// - [`DontYeetWalletError::Network`] for unknown `chain_id` or HTTP failure.
/// - [`DontYeetWalletError::Chain`] if the JSON can't be re-parsed.
pub async fn broadcast_signed(chain_id: &str, signed: &[u8]) -> Result<String> {
    if chain_id != "kaspa" {
        return Err(DontYeetWalletError::Network(format!(
            "in-browser Kaspa broadcaster does not handle {chain_id}"
        )));
    }

    let tx_json: serde_json::Value = serde_json::from_slice(signed)
        .map_err(|e| DontYeetWalletError::Chain(format!("tx JSON re-parse: {e}")))?;

    let url = format!("{KASPA_MAINNET}/transactions");
    let resp = Request::post(&url)
        .json(&tx_json)
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

    Ok(body.transaction_id)
}

/// Sign and broadcast a native KAS transfer entirely client-side.
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

/// Fetch UTXOs for the sender via `GET /addresses/{addr}/utxos`.
async fn fetch_utxos(address: &str) -> Result<Vec<KaspaUtxoResponse>> {
    let encoded_address = address.replace(':', "%3A");
    let url = format!("{KASPA_MAINNET}/addresses/{encoded_address}/utxos");

    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("UTXO fetch failed: {e}")))?;

    if !resp.ok() {
        return Err(DontYeetWalletError::Network(format!(
            "{url} returned status {}",
            resp.status()
        )));
    }

    resp.json()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("UTXO parse: {e}")))
}

// ---------------------------------------------------------------------------
// Response shapes
// ---------------------------------------------------------------------------

/// JSON response from `GET /addresses/{address}/balance`.
#[derive(Deserialize)]
struct BalanceResponse {
    /// Balance in SOMPI.
    balance: u64,
}

/// One element of `GET /addresses/{address}/utxos`.
#[derive(Deserialize)]
struct KaspaUtxoResponse {
    outpoint: KaspaOutpoint,
    #[serde(rename = "utxoEntry")]
    utxo_entry: KaspaUtxoEntry,
}

#[derive(Deserialize)]
struct KaspaOutpoint {
    #[serde(rename = "transactionId")]
    transaction_id: String,
    index: u32,
}

#[derive(Deserialize)]
struct KaspaUtxoEntry {
    amount: String,
    #[serde(rename = "scriptPublicKey")]
    script_public_key: KaspaScriptPubKey,
}

#[derive(Deserialize)]
struct KaspaScriptPubKey {
    #[serde(rename = "scriptPublicKey")]
    script_public_key: String,
}

/// JSON response from `POST /transactions`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BroadcastResponse {
    transaction_id: String,
}

// Rust guideline compliant 2026-02-21
