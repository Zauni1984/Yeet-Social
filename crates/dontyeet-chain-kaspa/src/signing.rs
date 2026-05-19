//! Pure-crypto Kaspa transfer encoding and ECDSA signing.
//!
//! Builds and signs a Kaspa transaction in JSON form (the shape the
//! public Kaspa REST API at `POST /transactions` expects). Both
//! server-side [`crate::transfer`] and the in-browser
//! [`crate::wasm::send`] pre-fetch UTXOs from the REST API, hand the
//! lot to [`build_signed_transfer`], and broadcast the returned
//! JSON bytes.
//!
//! Lives outside the `feature = "rpc"` gate so browser consumers
//! (`default-features = false`) can sign without pulling in
//! `reqwest`, `tokio`, or any other server-only dependency. Phase
//! M.4.7 mirrors the M.4.x split: extract the protocol logic into a
//! shared module, then have both the rpc-feature glue and the wasm
//! glue delegate to it.
//!
//! ## Implementation note
//!
//! Kaspa natively uses Schnorr (BIP-340) signatures and a specific
//! transaction-id hash computation. The current pipeline emits
//! ECDSA-over-SHA-256 64-byte sigs and a simplified sighash preimage,
//! mirroring what `tx.rs` was doing before this extraction. Replacing
//! both with proper Schnorr + the full Kaspa transaction-id hash is
//! tracked separately and, like the TRON 65-byte fix, is a
//! protocol-level correctness change rather than a WASM-migration
//! one.
//!
//! ## Layout
//!
//! - [`Utxo`] — RPC-shape-agnostic UTXO record (txid hex, vout,
//!   sompi value, hex script).
//! - [`build_signed_transfer`] — full pipeline: greedy coin select
//!   → outputs (recipient + change) → per-input sighash + sign →
//!   assemble transaction JSON.
//! - [`sign_input`] — SHA-256 + secp256k1 ECDSA → 64-byte (`r || s`)
//!   compact signature primitive, re-used by
//!   [`crate::tx::KaspaTransactionSigner`].
//! - [`address_to_script`] — `kaspa:`/`kaspatest:` address →
//!   `OP_DATA_20 || hash || OP_CHECKSIG` hex-encoded script.

use k256::ecdsa::{RecoveryId, Signature, SigningKey, signature::hazmat::PrehashSigner};
use sha2::{Digest, Sha256};

use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::PrivateKey;

/// Standard `subnetworkId` for built-in (non-registry) transactions.
const SUBNETWORK_NATIVE: &str = "0000000000000000000000000000000000000000";

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Minimal RPC-shape-agnostic Kaspa UTXO record.
///
/// Both `GET /addresses/{addr}/utxos` (server, REST API) and the
/// in-browser equivalent convert their responses to this shape
/// before reaching [`build_signed_transfer`].
#[derive(Clone, Debug)]
pub struct Utxo {
    /// Source transaction id (hex).
    pub txid: String,
    /// Output index inside the source transaction.
    pub index: u32,
    /// Value in sompi (1 KAS = 1e8 sompi).
    pub value_sompi: u64,
    /// Hex-encoded `scriptPublicKey`.
    pub script_hex: String,
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Build and sign a Kaspa native KAS transfer.
///
/// Pipeline: greedy coin select → outputs (recipient + optional
/// change) → per-input simplified sighash + ECDSA → assemble the
/// transaction JSON. Returns the JSON bytes ready for the broadcaster
/// to ship via `POST /transactions`.
///
/// `from_address` and `to_address` must use the `kaspa:` (or
/// `kaspatest:`) prefix — the routine extracts the 20-byte hash from
/// each and wraps it in a P2PK-like script.
///
/// # Errors
/// - [`DontYeetWalletError::Validation`] if `confirmed_utxos` is empty,
///   selected funds can't cover `amount + fee`, or addresses lack
///   a known prefix.
/// - [`DontYeetWalletError::Chain`] if the address hex is malformed.
/// - [`DontYeetWalletError::Crypto`] if signing fails.
pub fn build_signed_transfer(
    confirmed_utxos: &[Utxo],
    amount_sompi: u64,
    fee_sompi: u64,
    from_address: &str,
    to_address: &str,
    private_key: &PrivateKey,
) -> Result<Vec<u8>> {
    let target = amount_sompi
        .checked_add(fee_sompi)
        .ok_or_else(|| DontYeetWalletError::Validation("amount + fee overflow".into()))?;
    let (selected, total_input) = select_utxos(confirmed_utxos, target)?;

    let to_script = address_to_script(to_address)?;
    let from_script = address_to_script(from_address)?;
    let change = total_input - target;

    let mut outputs = vec![serde_json::json!({
        "amount": amount_sompi,
        "scriptPublicKey": { "version": 0, "scriptPublicKey": to_script }
    })];
    if change > 0 {
        outputs.push(serde_json::json!({
            "amount": change,
            "scriptPublicKey": { "version": 0, "scriptPublicKey": from_script }
        }));
    }

    let outputs_hash = hash_outputs(&outputs);

    let mut inputs = Vec::with_capacity(selected.len());
    for utxo in &selected {
        let preimage = build_sighash_preimage(utxo, &outputs_hash);
        let signature = sign_input(&preimage, private_key)?;

        // signatureScript: OP_DATA_65 (0x41) + 64-byte sig + SIGHASH_ALL (0x01).
        let mut sig_script = Vec::with_capacity(66);
        sig_script.push(0x41);
        sig_script.extend_from_slice(&signature);
        sig_script.push(0x01);

        inputs.push(serde_json::json!({
            "previousOutpoint": {
                "transactionId": utxo.txid,
                "index": utxo.index,
            },
            "signatureScript": hex::encode(&sig_script),
            "sequence": 0,
            "sigOpCount": 1,
        }));
    }

    let tx = serde_json::json!({
        "transaction": {
            "version": 0,
            "inputs": inputs,
            "outputs": outputs,
            "lockTime": "0",
            "subnetworkId": SUBNETWORK_NATIVE,
        }
    });

    serde_json::to_vec(&tx).map_err(|e| DontYeetWalletError::Chain(format!("JSON serialize: {e}")))
}

/// Sign one Kaspa sighash preimage and return the 64-byte signature.
///
/// SHA-256s the preimage and signs with secp256k1 ECDSA. Returns the
/// 64-byte compact form (`r || s`); the recovery byte is dropped to
/// match what the existing pipeline emits. Used by both
/// [`build_signed_transfer`] (per-input) and the
/// [`crate::tx::KaspaTransactionSigner`] trait impl that satisfies
/// `ChainPlugin::signer()`.
///
/// # Errors
/// Returns [`DontYeetWalletError::Crypto`] if `private_key` is rejected by
/// secp256k1 or signing fails.
pub fn sign_input(preimage: &[u8], private_key: &PrivateKey) -> Result<Vec<u8>> {
    let hash = Sha256::digest(preimage);

    let signing_key = SigningKey::from_slice(private_key.as_bytes())
        .map_err(|e| DontYeetWalletError::Crypto(format!("invalid secp256k1 key: {e}")))?;

    let (signature, _recovery_id): (Signature, RecoveryId) = signing_key
        .sign_prehash(hash.as_slice())
        .map_err(|e| DontYeetWalletError::Crypto(format!("Kaspa signing failed: {e}")))?;

    Ok(signature.to_bytes().to_vec())
}

/// Convert a `kaspa:` / `kaspatest:` address into a hex-encoded script.
///
/// Wraps the address's 20-byte hash in `OP_DATA_20 (0x14) || hash ||
/// OP_CHECKSIG (0xac)`. Returns the result as a hex string ready to
/// drop into the `scriptPublicKey` JSON field.
///
/// # Errors
/// - [`DontYeetWalletError::Chain`] if the address lacks a known Kaspa
///   prefix or the hash portion isn't valid hex.
pub fn address_to_script(address: &str) -> Result<String> {
    let hex_part = address
        .strip_prefix("kaspa:")
        .or_else(|| address.strip_prefix("kaspatest:"))
        .ok_or_else(|| DontYeetWalletError::Chain("invalid Kaspa address prefix".into()))?;
    hex::decode(hex_part).map_err(|e| DontYeetWalletError::Chain(format!("address hex decode: {e}")))?;
    Ok(format!("14{hex_part}ac"))
}

// ---------------------------------------------------------------------------
// Internal: coin selection + sighash + helpers
// ---------------------------------------------------------------------------

/// Greedy descending coin selection.
///
/// Returns the selected UTXOs plus the total input value. Picks the
/// highest-value UTXO first and accumulates until the running total
/// covers `target_sompi`.
fn select_utxos(utxos: &[Utxo], target_sompi: u64) -> Result<(Vec<&Utxo>, u64)> {
    let mut indexed: Vec<(u64, usize)> = utxos
        .iter()
        .enumerate()
        .map(|(i, u)| (u.value_sompi, i))
        .collect();
    indexed.sort_unstable_by(|a, b| b.0.cmp(&a.0));

    let mut selected: Vec<&Utxo> = Vec::new();
    let mut total: u64 = 0;
    for (value, idx) in &indexed {
        selected.push(&utxos[*idx]);
        total = total.saturating_add(*value);
        if total >= target_sompi {
            return Ok((selected, total));
        }
    }
    Err(DontYeetWalletError::Validation(format!(
        "insufficient funds: have {total} sompi, need {target_sompi}"
    )))
}

/// Build a simplified sighash preimage for one input.
///
/// Combines the outpoint, UTXO value, script, and outputs hash into
/// a deterministic byte sequence for signing. Mirrors what the
/// pre-extraction `transfer::build_sighash_preimage` produced.
fn build_sighash_preimage(utxo: &Utxo, outputs_hash: &[u8; 32]) -> Vec<u8> {
    let mut preimage = Vec::with_capacity(128);

    if let Ok(txid_bytes) = hex::decode(&utxo.txid) {
        preimage.extend_from_slice(&txid_bytes);
    }
    preimage.extend_from_slice(&utxo.index.to_le_bytes());
    preimage.extend_from_slice(&utxo.value_sompi.to_le_bytes());
    if let Ok(script_bytes) = hex::decode(&utxo.script_hex) {
        preimage.extend_from_slice(&script_bytes);
    }
    preimage.extend_from_slice(outputs_hash);

    preimage
}

/// SHA-256 of the serialized outputs JSON, used in every per-input
/// sighash preimage.
fn hash_outputs(outputs: &[serde_json::Value]) -> [u8; 32] {
    let serialized = serde_json::to_vec(outputs).unwrap_or_default();
    let hash = Sha256::digest(&serialized);
    let mut result = [0u8; 32];
    result.copy_from_slice(&hash);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_key() -> PrivateKey {
        let key_bytes =
            hex::decode("4c0883a69102937d6231471b5dbb6204fe512961708279f16c4b0e0a9ab8024a")
                .expect("hex");
        PrivateKey::new(key_bytes)
    }

    fn make_utxo(txid: &str, index: u32, value: u64) -> Utxo {
        Utxo {
            txid: txid.into(),
            index,
            value_sompi: value,
            script_hex: "14aabbccdd00112233445566778899aabbccddee00ac".into(),
        }
    }

    #[test]
    fn address_to_script_mainnet() {
        let addr = format!("kaspa:{}", "ab".repeat(20));
        let script = address_to_script(&addr).expect("script");
        assert!(script.starts_with("14"));
        assert!(script.ends_with("ac"));
        assert_eq!(script.len(), 2 + 40 + 2);
    }

    #[test]
    fn address_to_script_testnet() {
        let addr = format!("kaspatest:{}", "cd".repeat(20));
        let script = address_to_script(&addr).expect("script");
        assert!(script.starts_with("14"));
    }

    #[test]
    fn address_to_script_rejects_unknown_prefix() {
        assert!(address_to_script("bitcoin:1234").is_err());
    }

    #[test]
    fn select_utxos_picks_largest_first() {
        let utxos = vec![
            make_utxo("aa", 0, 5_000),
            make_utxo("bb", 0, 3_000),
            make_utxo("cc", 0, 10_000),
        ];
        let (selected, total) = select_utxos(&utxos, 8_000).expect("select");
        assert_eq!(selected.len(), 1);
        assert_eq!(total, 10_000);
    }

    #[test]
    fn select_utxos_insufficient() {
        let utxos = vec![make_utxo("aa", 0, 100)];
        assert!(matches!(
            select_utxos(&utxos, 50_000),
            Err(DontYeetWalletError::Validation(_))
        ));
    }

    #[test]
    fn select_utxos_exact() {
        let utxos = vec![make_utxo("aa", 0, 5_000)];
        let (selected, total) = select_utxos(&utxos, 5_000).expect("select");
        assert_eq!(total, 5_000);
        assert_eq!(selected.len(), 1);
    }

    #[test]
    fn sign_input_returns_64_bytes() {
        let pk = fixture_key();
        let sig = sign_input(b"fake preimage", &pk).expect("sign");
        assert_eq!(sig.len(), 64);
    }

    #[test]
    fn sign_input_is_deterministic() {
        let pk = fixture_key();
        let s1 = sign_input(b"deterministic", &pk).expect("s1");
        let s2 = sign_input(b"deterministic", &pk).expect("s2");
        assert_eq!(s1, s2);
    }

    #[test]
    fn sign_input_rejects_short_key() {
        let pk = PrivateKey::new(vec![0u8; 16]);
        assert!(sign_input(b"data", &pk).is_err());
    }

    #[test]
    fn build_signed_transfer_round_trip() {
        let pk = fixture_key();
        let utxos = vec![make_utxo(&"aa".repeat(32), 0, 100_000)];
        let from = format!("kaspa:{}", "ab".repeat(20));
        let to = format!("kaspa:{}", "cd".repeat(20));

        let bytes = build_signed_transfer(&utxos, 50_000, 3_000, &from, &to, &pk).expect("build");

        // Result must be a JSON object with a top-level "transaction".
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).expect("valid JSON");
        assert!(parsed.get("transaction").is_some());
        let tx = &parsed["transaction"];
        assert_eq!(tx["inputs"].as_array().expect("array").len(), 1);
        // Two outputs: recipient + change.
        assert_eq!(tx["outputs"].as_array().expect("array").len(), 2);
    }

    #[test]
    fn build_signed_transfer_skips_change_when_zero() {
        let pk = fixture_key();
        // Exact funds: amount + fee = total UTXO.
        let utxos = vec![make_utxo(&"aa".repeat(32), 0, 53_000)];
        let from = format!("kaspa:{}", "ab".repeat(20));
        let to = format!("kaspa:{}", "cd".repeat(20));

        let bytes = build_signed_transfer(&utxos, 50_000, 3_000, &from, &to, &pk).expect("build");
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).expect("valid JSON");
        assert_eq!(
            parsed["transaction"]["outputs"]
                .as_array()
                .expect("array")
                .len(),
            1
        );
    }

    #[test]
    fn build_signed_transfer_rejects_insufficient() {
        let pk = fixture_key();
        let utxos = vec![make_utxo(&"aa".repeat(32), 0, 100)];
        let from = format!("kaspa:{}", "ab".repeat(20));
        let to = format!("kaspa:{}", "cd".repeat(20));
        assert!(matches!(
            build_signed_transfer(&utxos, 50_000, 3_000, &from, &to, &pk),
            Err(DontYeetWalletError::Validation(_))
        ));
    }
}

// Rust guideline compliant 2026-02-21
