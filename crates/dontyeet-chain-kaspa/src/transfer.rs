//! Server-side Kaspa native KAS transfer.
//!
//! Fetches UTXOs from the Kaspa REST API, converts them into the
//! RPC-shape-agnostic [`signing::Utxo`] form, and hands the lot to
//! [`signing::build_signed_transfer`] for the pure-crypto pipeline
//! (coin selection, sighash + ECDSA, JSON assembly). Phase M.4.7
//! moved everything after the network call into the always-on
//! [`crate::signing`] module so the in-browser send pipeline goes
//! through the same code.

use std::collections::HashMap;

use url::Url;

use dontyeet_network::encode_segment;
use dontyeet_primitives::address::Address;
use dontyeet_primitives::amount::Amount;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::PrivateKey;

use crate::rest;
use crate::signing::{self, Utxo};

/// Default fee: 1 sompi/gram × 3000 grams (simple-transfer mass).
const DEFAULT_FEE_SOMPI: u64 = 3000;

// ---- UTXO response shapes ----

#[derive(serde::Deserialize)]
struct KaspaUtxoResponse {
    outpoint: KaspaOutpoint,
    #[serde(rename = "utxoEntry")]
    utxo_entry: KaspaUtxoEntry,
}

#[derive(serde::Deserialize)]
struct KaspaOutpoint {
    #[serde(rename = "transactionId")]
    transaction_id: String,
    index: u32,
}

#[derive(serde::Deserialize)]
struct KaspaUtxoEntry {
    amount: String,
    #[serde(rename = "scriptPublicKey")]
    script_public_key: KaspaScriptPubKey,
}

#[derive(serde::Deserialize)]
struct KaspaScriptPubKey {
    #[serde(rename = "scriptPublicKey")]
    script_public_key: String,
}

// ---- Public API ----

/// Build and sign a native KAS transfer.
///
/// Fetches the sender's UTXOs from the Kaspa REST API, then delegates
/// to [`signing::build_signed_transfer`] for greedy coin selection +
/// per-input signing + JSON assembly. Returns the JSON bytes ready
/// for the broadcaster to ship to `POST /transactions`.
///
/// # Errors
/// Returns [`DontYeetWalletError`] if the API URL list is missing, UTXO
/// fetch fails, address decoding fails, funds are insufficient, or
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
    let sompi = u64::try_from(amount.raw())
        .map_err(|_| DontYeetWalletError::Validation("amount too large for KAS transfer".into()))?;

    let urls = api_urls
        .get(network)
        .ok_or_else(|| DontYeetWalletError::NotFound(format!("no API URLs for {network}")))?;

    let raw = fetch_utxos(urls, from).await?;

    let utxos: Vec<Utxo> = raw
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
        from.as_str(),
        to.as_str(),
        private_key,
    )
}

// ---- UTXO fetching ----

/// Fetch unspent transaction outputs for an address.
async fn fetch_utxos(urls: &[Url], address: &Address) -> Result<Vec<KaspaUtxoResponse>> {
    let path = format!("addresses/{}/utxos", encode_segment(address.as_str()));
    rest::rest_get(urls, &path).await
}

// Rust guideline compliant 2026-02-21
