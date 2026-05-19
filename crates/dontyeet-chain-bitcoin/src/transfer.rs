//! Server-side Bitcoin native BTC transfer — UTXO/fee fetch then [`signing`].
//!
//! Fetches confirmed UTXOs and fee rate from Mempool.space, hands the
//! lot to [`crate::signing::build_signed_transfer`], and returns the
//! raw signed segwit transaction bytes for the broadcaster. Phase
//! M.4.2 moved everything after the network calls into the always-on
//! [`crate::signing`] module so the in-browser send pipeline goes
//! through the same code.

use std::collections::HashMap;

use url::Url;

use dontyeet_network::{HttpClient, ReqwestClient, encode_segment};
use dontyeet_primitives::address::Address;
use dontyeet_primitives::amount::Amount;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::PrivateKey;

use crate::keys::compressed_pubkey;
use crate::signing::{self, Utxo};

/// Default fee rate (sats/vbyte) when the fee estimation API fails.
const DEFAULT_FEE_RATE: u64 = 5;

// ---- Mempool.space response shapes ----

#[derive(serde::Deserialize)]
struct MempoolUtxo {
    txid: String,
    vout: u32,
    value: u64,
    status: MempoolUtxoStatus,
}

#[derive(serde::Deserialize)]
struct MempoolUtxoStatus {
    confirmed: bool,
}

#[derive(serde::Deserialize)]
struct FeeEstimates {
    #[serde(rename = "halfHourFee")]
    half_hour_fee: u64,
}

// ---- Public API ----

/// Build and sign a native BTC transfer (P2WPKH segwit).
///
/// Fetches confirmed UTXOs and the half-hour fee rate from
/// Mempool.space, then calls [`signing::build_signed_transfer`] for
/// the pure-crypto pipeline (coin selection, BIP-143, ECDSA,
/// segwit serialization). Returns the raw transaction bytes ready
/// for the broadcaster.
///
/// # Errors
/// Returns [`DontYeetWalletError`] if the API URL list is missing for
/// `network`, UTXO/fee fetch fails, the recipient address isn't a
/// valid bech32 P2WPKH, the amount is too large for `u64`, or
/// signing fails (insufficient funds, bad key).
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
    let amount_sats = u64::try_from(amount.raw())
        .map_err(|_| DontYeetWalletError::Validation("amount too large for BTC transfer".into()))?;

    let urls = api_urls
        .get(network)
        .ok_or_else(|| DontYeetWalletError::NotFound(format!("no API URLs for {network}")))?;
    let base = urls
        .first()
        .ok_or_else(|| DontYeetWalletError::NotFound("API URL list is empty".into()))?;

    // 1. Network I/O.
    let mempool_utxos = fetch_utxos(base, from).await?;
    let fee_rate = fetch_fee_rate(base).await.unwrap_or(DEFAULT_FEE_RATE);

    // 2. Convert into the RPC-shape-agnostic form `signing` expects.
    let utxos: Vec<Utxo> = mempool_utxos
        .into_iter()
        .map(|u| Utxo {
            txid: u.txid,
            vout: u.vout,
            value_sats: u.value,
        })
        .collect();

    // 3. Decode recipient + derive sender pubkey (pure crypto).
    let recipient = signing::decode_p2wpkh_address(to.as_str())?;
    let sender_pubkey = compressed_pubkey(private_key)?;

    // 4. Build + sign in one shot via the shared signing core.
    let signed = signing::build_signed_transfer(
        &utxos,
        fee_rate,
        &sender_pubkey,
        &recipient,
        private_key,
        amount_sats,
    )?;

    Ok(signed.raw_tx)
}

// ---- UTXO + fee fetching (Mempool.space) ----

/// Fetch confirmed UTXOs from Mempool.space.
async fn fetch_utxos(base: &Url, address: &Address) -> Result<Vec<MempoolUtxo>> {
    let url = base
        .join(&format!(
            "/api/address/{}/utxo",
            encode_segment(address.as_str())
        ))
        .map_err(|e| DontYeetWalletError::Network(format!("URL join: {e}")))?;

    let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
    let resp = client
        .get(&url)
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("UTXO fetch: {e}")))?;

    let all: Vec<MempoolUtxo> = resp
        .json()
        .map_err(|e| DontYeetWalletError::Network(format!("UTXO parse: {e}")))?;

    Ok(all.into_iter().filter(|u| u.status.confirmed).collect())
}

/// Fetch the half-hour fee rate (sats/vbyte) from Mempool.space.
async fn fetch_fee_rate(base: &Url) -> Result<u64> {
    let url = base
        .join("/api/v1/fees/recommended")
        .map_err(|e| DontYeetWalletError::Network(format!("URL join: {e}")))?;

    let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
    let resp = client
        .get(&url)
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("fee fetch: {e}")))?;

    let estimates: FeeEstimates = resp
        .json()
        .map_err(|e| DontYeetWalletError::Network(format!("fee parse: {e}")))?;

    Ok(estimates.half_hour_fee)
}

// Rust guideline compliant 2026-02-21
