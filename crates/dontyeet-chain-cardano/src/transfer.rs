//! Server-side Cardano native ADA transfer (Blockfrost).
//!
//! Fetches UTXOs, protocol parameters, and the latest slot from
//! Blockfrost, converts them into the RPC-shape-agnostic
//! [`signing::Utxo`] form, and hands the lot to
//! [`signing::build_signed_transfer`] for the pure-crypto pipeline
//! (greedy coin selection, CBOR encoding, Blake2b-256 + Ed25519,
//! re-encode with witness set). Phase M.4.9 moved everything after
//! the network calls into the always-on [`crate::signing`] module
//! so the in-browser send pipeline (which uses Koios instead of
//! Blockfrost) goes through the same code.

use std::collections::HashMap;

use url::Url;

use dontyeet_network::{HttpClient, ReqwestClient, encode_segment};
use dontyeet_primitives::address::Address;
use dontyeet_primitives::amount::Amount;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::PrivateKey;

use crate::keys::ed25519_pubkey;
use crate::signing::{self, Utxo};

// ---- Blockfrost response shapes ----

#[derive(serde::Deserialize)]
struct BlockfrostUtxo {
    tx_hash: String,
    output_index: u32,
    amount: Vec<BlockfrostAmount>,
}

#[derive(serde::Deserialize)]
struct BlockfrostAmount {
    unit: String,
    quantity: String,
}

#[derive(serde::Deserialize)]
struct BlockfrostBlock {
    slot: u64,
}

#[derive(serde::Deserialize)]
struct ProtocolParams {
    min_fee_a: u64,
    min_fee_b: u64,
}

// ---- Public API ----

/// Build and sign a native ADA payment transaction.
///
/// Fetches UTXOs (filtered to lovelace-only), protocol parameters,
/// and the latest slot from Blockfrost, then delegates to
/// [`signing::build_signed_transfer`] for the pure-crypto pipeline.
/// Returns the full CBOR-encoded transaction ready for
/// `tx/submit`.
///
/// # Errors
/// Returns [`DontYeetWalletError`] if the API URL list is missing for
/// `network`, the Blockfrost calls fail, address decoding fails,
/// funds are insufficient, or signing fails.
#[expect(
    clippy::implicit_hasher,
    reason = "callers always pass HashMap with default RandomState; generic hasher would not improve API ergonomics"
)]
pub async fn build_signed_transfer(
    api_urls: &HashMap<NetworkId, Vec<Url>>,
    project_id: Option<&str>,
    from: &Address,
    to: &Address,
    amount: &Amount,
    private_key: &PrivateKey,
    network: &NetworkId,
) -> Result<Vec<u8>> {
    let lovelace = u64::try_from(amount.raw())
        .map_err(|_| DontYeetWalletError::Validation("amount too large for ADA transfer".into()))?;

    let urls = api_urls
        .get(network)
        .ok_or_else(|| DontYeetWalletError::NotFound(format!("no API URLs for {network}")))?;
    let base = urls
        .first()
        .ok_or_else(|| DontYeetWalletError::NotFound("API URL list is empty".into()))?;

    // 1. Pure crypto: bech32-decode the addresses + derive sender
    //    pubkey.
    let from_bytes = signing::decode_bech32_address(from.as_str())?;
    let to_bytes = signing::decode_bech32_address(to.as_str())?;
    let sender_pubkey = ed25519_pubkey(private_key)?;

    // 2. Network: UTXOs + protocol params + latest slot.
    let raw_utxos = fetch_utxos(base, project_id, from).await?;
    let params = fetch_protocol_params(base, project_id).await?;
    let latest_slot = fetch_latest_slot(base, project_id).await?;

    // 3. Convert Blockfrost shape into the signing module's form.
    let utxos: Vec<Utxo> = raw_utxos
        .into_iter()
        .filter_map(|u| {
            let lovelace_str = u
                .amount
                .iter()
                .find(|a| a.unit == "lovelace")
                .map(|a| a.quantity.clone())?;
            let value = lovelace_str.parse::<u64>().ok()?;
            Some(Utxo {
                tx_hash: u.tx_hash,
                output_index: u.output_index,
                lovelace: value,
            })
        })
        .collect();

    // 4. Build + sign via the shared signing core.
    signing::build_signed_transfer(
        &utxos,
        params.min_fee_a,
        params.min_fee_b,
        latest_slot,
        &from_bytes,
        &to_bytes,
        &sender_pubkey,
        lovelace,
        private_key,
    )
}

// ---- Blockfrost API calls ----

/// Fetch UTXOs for an address from Blockfrost.
async fn fetch_utxos(
    base: &Url,
    project_id: Option<&str>,
    address: &Address,
) -> Result<Vec<BlockfrostUtxo>> {
    let url = base
        .join(&format!(
            "/api/v0/addresses/{}/utxos",
            encode_segment(address.as_str())
        ))
        .map_err(|e| DontYeetWalletError::Network(format!("URL join: {e}")))?;

    let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
    let headers = crate::auth::project_id_headers(project_id);
    let resp = client
        .get_with_headers(&url, &headers)
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("UTXO fetch: {e}")))?;

    resp.json()
        .map_err(|e| DontYeetWalletError::Network(format!("UTXO parse: {e}")))
}

/// Fetch protocol parameters from Blockfrost.
async fn fetch_protocol_params(base: &Url, project_id: Option<&str>) -> Result<ProtocolParams> {
    let url = base
        .join("/api/v0/epochs/latest/parameters")
        .map_err(|e| DontYeetWalletError::Network(format!("URL join: {e}")))?;

    let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
    let headers = crate::auth::project_id_headers(project_id);
    let resp = client
        .get_with_headers(&url, &headers)
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("params fetch: {e}")))?;

    resp.json()
        .map_err(|e| DontYeetWalletError::Network(format!("params parse: {e}")))
}

/// Fetch the latest block's slot number from Blockfrost.
async fn fetch_latest_slot(base: &Url, project_id: Option<&str>) -> Result<u64> {
    let url = base
        .join("/api/v0/blocks/latest")
        .map_err(|e| DontYeetWalletError::Network(format!("URL join: {e}")))?;

    let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
    let headers = crate::auth::project_id_headers(project_id);
    let resp = client
        .get_with_headers(&url, &headers)
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("block fetch: {e}")))?;

    let block: BlockfrostBlock = resp
        .json()
        .map_err(|e| DontYeetWalletError::Network(format!("block parse: {e}")))?;

    Ok(block.slot)
}

// Rust guideline compliant 2026-02-21
