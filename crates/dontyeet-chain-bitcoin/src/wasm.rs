//! In-browser UTXO balance reads and transaction signing.
//!
//! Two entry points, both browser-only:
//!
//! - [`fetch_balance`] — Phase M.3.2.2. Calls
//!   `GET {base}/address/{addr}` directly from the WASM bundle, no
//!   server hop.
//! - [`send`] — Phase M.4.2. End-to-end client-side send pipeline:
//!   derive keypair → fetch UTXOs → fetch fee rate → coin-select +
//!   sign (via [`crate::signing`]) → broadcast (`POST {base}/tx`).
//!
//! Lives outside the [`feature = "rpc"`] gate because `gloo_net` is
//! fundamentally a browser API; the whole module is `#[cfg]`-gated to
//! `wasm32` targets so it never enters native server builds.
//!
//! Two chains share the mempool.space-flavoured REST shape:
//!
//! - **bitcoin** → `https://mempool.space/api`
//! - **litecoin** → `https://litecoinspace.org/api` (mempool.space fork)
//!
//! Mainnet only — testnet/devnet calls fall back to the server-proxied
//! Path A endpoint.

use serde::Deserialize;

use gloo_net::http::Request;

use dontyeet_crypto::Bip44Deriver;
use dontyeet_primitives::secret::Seed;
use dontyeet_primitives::{Amount, DontYeetWalletError, Result};

use crate::keys::compressed_pubkey;
use crate::signing::{self, Utxo};

/// Both Bitcoin and Litecoin denominate balances in 1e-8 of the
/// native unit (1 BTC = `100_000_000` sats; 1 LTC = `100_000_000` lits).
const UTXO_DECIMALS: u8 = 8;

/// Default fee rate (sats/vbyte) when the recommended-fees endpoint
/// is unreachable. Mirrors `transfer::DEFAULT_FEE_RATE` server-side.
const DEFAULT_FEE_RATE: u64 = 5;

/// Mainnet REST API base for the supported UTXO chains.
///
/// Returns `None` for any chain not in the in-browser table — the
/// caller falls back to Path A for those.
fn mainnet_api_base(chain_id: &str) -> Option<&'static str> {
    Some(match chain_id {
        "bitcoin" => "https://mempool.space/api",
        "litecoin" => "https://litecoinspace.org/api",
        _ => return None,
    })
}

/// BIP-84 derivation path for the supported UTXO chains.
///
/// Bitcoin uses coin type 0 (`m/84'/0'/0'/0/0`); Litecoin uses coin
/// type 2 (`m/84'/2'/0'/0/0`). Mirrors the constants in
/// `crates/dontyeet-ui/src/wallet/addresses.rs` so the address the
/// browser displays and the address it spends from stay aligned.
fn derivation_path(chain_id: &str) -> Option<&'static str> {
    Some(match chain_id {
        "bitcoin" => "m/84'/0'/0'/0/0",
        "litecoin" => "m/84'/2'/0'/0/0",
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Fetch the on-chain balance for `address` on `chain_id` mainnet.
///
/// Calls `GET {base}/address/{address}` against the chain's
/// mempool.space-compatible REST endpoint and computes
/// `funded_txo_sum - spent_txo_sum` in the native sub-unit
/// (satoshis / litoshis).
///
/// # Errors
/// - [`DontYeetWalletError::Network`] if the chain isn't a built-in UTXO
///   chain, the HTTP call fails (CORS rejection, transport error,
///   non-2xx status), or the body can't be parsed.
/// - [`DontYeetWalletError::Chain`] if `spent_txo_sum > funded_txo_sum`,
///   which would indicate a corrupt indexer response.
pub async fn fetch_balance(chain_id: &str, address: &str) -> Result<Amount> {
    let base = mainnet_api_base(chain_id).ok_or_else(|| {
        DontYeetWalletError::Network(format!("no in-browser API base configured for {chain_id}"))
    })?;

    // BTC and LTC addresses are bech32 or base58 — both alphanumeric,
    // no URL-reserved characters — so direct interpolation is safe.
    let url = format!("{base}/address/{address}");

    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("address fetch failed: {e}")))?;

    if !resp.ok() {
        return Err(DontYeetWalletError::Network(format!(
            "{url} returned status {}",
            resp.status()
        )));
    }

    let info: AddressInfo = resp
        .json()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("address info parse: {e}")))?;

    let sats = info
        .chain_stats
        .funded_txo_sum
        .checked_sub(info.chain_stats.spent_txo_sum)
        .ok_or_else(|| DontYeetWalletError::Chain("balance underflow: spent > funded".into()))?;

    Ok(Amount::from_raw(u128::from(sats), UTXO_DECIMALS))
}

/// Build a signed UTXO transfer transaction without broadcasting.
///
/// Pipeline: derive keypair → decode recipient → fetch UTXOs + fee
/// rate → call [`signing::build_signed_transfer`] for greedy
/// coin-select + BIP-143 sighash + ECDSA + segwit serialization.
/// Returns the raw signed transaction bytes ready for `POST {base}/tx`.
///
/// Splitting build from broadcast lets the orchestration layer hash
/// the signed bytes for deduplication caching.
///
/// # Errors
/// - [`DontYeetWalletError::Network`] for any HTTP failure or unknown `chain_id`.
/// - [`DontYeetWalletError::Validation`] if `amount_raw` doesn't parse as
///   a `u64`, the recipient isn't a valid bech32 P2WPKH, the
///   sender's compressed pubkey can't be derived, or selected funds
///   don't cover `amount + fee`.
/// - [`DontYeetWalletError::Crypto`] if BIP-44 derivation or ECDSA signing fails.
pub async fn build_signed_payload(
    chain_id: &str,
    seed: &Seed,
    to: &str,
    amount_raw: &str,
) -> Result<Vec<u8>> {
    let base = mainnet_api_base(chain_id).ok_or_else(|| {
        DontYeetWalletError::Network(format!("no in-browser API base configured for {chain_id}"))
    })?;
    let path = derivation_path(chain_id).ok_or_else(|| {
        DontYeetWalletError::Network(format!("no derivation path configured for {chain_id}"))
    })?;

    let private_key = Bip44Deriver::derive(seed, path)?;
    let sender_pubkey = compressed_pubkey(&private_key)?;

    let recipient = signing::decode_p2wpkh_address(to)?;
    let amount_sats: u64 = amount_raw
        .parse()
        .map_err(|e| DontYeetWalletError::Validation(format!("invalid amount '{amount_raw}': {e}")))?;

    let sender_hash = crate::keys::hash160(&sender_pubkey);
    let sender_address = encode_p2wpkh(chain_id, &sender_hash)?;

    let utxos = fetch_utxos(base, &sender_address).await?;
    let fee_rate = fetch_fee_rate(base).await.unwrap_or(DEFAULT_FEE_RATE);

    let signed = signing::build_signed_transfer(
        &utxos,
        fee_rate,
        &sender_pubkey,
        &recipient,
        &private_key,
        amount_sats,
    )?;

    Ok(signed.raw_tx)
}

/// Broadcast a pre-signed raw UTXO transaction.
///
/// `POST {base}/tx` with the transaction hex-encoded in the body.
/// Returns the broadcast txid.
///
/// # Errors
/// - [`DontYeetWalletError::Network`] for unknown `chain_id` or any HTTP
///   transport / broadcast failure.
pub async fn broadcast_signed(chain_id: &str, signed: &[u8]) -> Result<String> {
    let base = mainnet_api_base(chain_id).ok_or_else(|| {
        DontYeetWalletError::Network(format!("no in-browser API base configured for {chain_id}"))
    })?;
    let raw_hex = hex::encode(signed);
    broadcast(base, &raw_hex).await
}

/// Sign and broadcast a native UTXO transfer entirely client-side.
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
// Address re-encoding (browser-side, mirrors keys::encode_p2wpkh_address)
// ---------------------------------------------------------------------------

/// Encode the 20-byte witness program as a `bech32` P2WPKH address
/// for the given chain.
///
/// Standalone helper instead of plumbing the `BtcAddressEncoder`
/// (which carries `BtcConfig` machinery the WASM bundle deliberately
/// avoids) — same `bech32::segwit::encode_v0` call under the hood.
fn encode_p2wpkh(chain_id: &str, witness_program: &[u8; 20]) -> Result<String> {
    let hrp_str = match chain_id {
        "bitcoin" => "bc",
        "litecoin" => "ltc",
        _ => {
            return Err(DontYeetWalletError::Network(format!(
                "no bech32 HRP configured for {chain_id}"
            )));
        }
    };
    let hrp = bech32::Hrp::parse(hrp_str)
        .map_err(|e| DontYeetWalletError::Chain(format!("invalid bech32 HRP: {e}")))?;
    bech32::segwit::encode_v0(hrp, witness_program)
        .map_err(|e| DontYeetWalletError::Chain(format!("bech32 encode error: {e}")))
}

// ---------------------------------------------------------------------------
// HTTP plumbing
// ---------------------------------------------------------------------------

/// Fetch confirmed UTXOs for `address` and convert to [`Utxo`].
async fn fetch_utxos(base: &str, address: &str) -> Result<Vec<Utxo>> {
    let url = format!("{base}/address/{address}/utxo");

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

    let raw: Vec<UtxoResponse> = resp
        .json()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("UTXO parse: {e}")))?;

    Ok(raw
        .into_iter()
        .filter(|u| u.status.confirmed)
        .map(|u| Utxo {
            txid: u.txid,
            vout: u.vout,
            value_sats: u.value,
        })
        .collect())
}

/// Fetch the half-hour fee rate from `/v1/fees/recommended`.
///
/// Returns `None` on any failure so the caller can fall back to
/// [`DEFAULT_FEE_RATE`] — same behavior as the server-side
/// `transfer::fetch_fee_rate` wrapped in `unwrap_or`.
async fn fetch_fee_rate(base: &str) -> Option<u64> {
    let url = format!("{base}/v1/fees/recommended");
    let resp = Request::get(&url).send().await.ok()?;
    if !resp.ok() {
        return None;
    }
    let est: FeeEstimates = resp.json().await.ok()?;
    Some(est.half_hour_fee)
}

/// Broadcast `raw_hex` via `POST {base}/tx`.
///
/// Mempool.space-compatible endpoints accept the raw hex transaction
/// as a plain-text body and return the txid as plain text on success.
async fn broadcast(base: &str, raw_hex: &str) -> Result<String> {
    let url = format!("{base}/tx");

    let resp = Request::post(&url)
        .header("Content-Type", "text/plain")
        .body(raw_hex.to_string())
        .map_err(|e| DontYeetWalletError::Network(format!("broadcast body build: {e}")))?
        .send()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("broadcast send: {e}")))?;

    if !resp.ok() {
        let status = resp.status();
        // Mempool.space returns the rejection reason as the body.
        let body_text = resp.text().await.unwrap_or_default();
        return Err(DontYeetWalletError::Network(format!(
            "broadcast {url} returned status {status}: {body_text}"
        )));
    }

    let txid = resp
        .text()
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("txid read: {e}")))?;
    Ok(txid.trim().to_string())
}

// ---------------------------------------------------------------------------
// Response shapes
// ---------------------------------------------------------------------------

/// Subset of the `/address/{addr}` response we care about.
#[derive(Deserialize)]
struct AddressInfo {
    chain_stats: ChainStats,
}

/// On-chain confirmed input/output sums in the native sub-unit.
#[derive(Deserialize)]
struct ChainStats {
    funded_txo_sum: u64,
    spent_txo_sum: u64,
}

/// Subset of one element of the `/address/{addr}/utxo` array.
#[derive(Deserialize)]
struct UtxoResponse {
    txid: String,
    vout: u32,
    value: u64,
    status: UtxoStatus,
}

/// Confirmation flag wrapper (mempool.space puts it in a nested
/// `status` object alongside block height / hash / time).
#[derive(Deserialize)]
struct UtxoStatus {
    confirmed: bool,
}

/// Subset of the `/v1/fees/recommended` response we care about.
#[derive(Deserialize)]
struct FeeEstimates {
    #[serde(rename = "halfHourFee")]
    half_hour_fee: u64,
}

// Rust guideline compliant 2026-02-21
