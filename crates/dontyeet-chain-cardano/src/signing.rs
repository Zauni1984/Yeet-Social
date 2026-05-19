//! Pure-crypto Cardano payment encoding and Ed25519 signing.
//!
//! Builds, signs, and serializes a Babbage-era ADA payment
//! transaction in CBOR form (the on-the-wire shape Cardano nodes
//! accept). Both server-side [`crate::transfer`] (Blockfrost) and
//! the in-browser [`crate::wasm::send`] (Koios) pre-fetch UTXOs +
//! protocol params + the latest slot, then call
//! [`build_signed_transfer`] to produce the bytes ready for
//! `submittx` / `tx/submit` broadcast.
//!
//! Lives outside the `feature = "rpc"` gate so browser consumers
//! (`default-features = false`) can sign without pulling in
//! `reqwest`, `tokio`, or any other server-only dependency. Phase
//! M.4.9 mirrors the M.4.x split: extract the protocol logic into
//! a shared module, then have both the rpc-feature glue and the
//! wasm glue delegate to it.
//!
//! ## Layout
//!
//! - [`Utxo`] — RPC-shape-agnostic UTXO record (txid hex, output
//!   index, lovelace value).
//! - [`build_signed_transfer`] — full pipeline: greedy coin select
//!   → CBOR-encoded transaction body → Blake2b-256 → Ed25519 →
//!   re-CBOR with witness-set + `is_valid: true` + null aux.
//! - [`sign_tx_body`] — Blake2b-256 + Ed25519 + post-sign verify
//!   primitive, re-used by [`crate::tx::CardanoTransactionSigner`]
//!   so the `ChainPlugin::signer()` path goes through identical
//!   code.
//! - [`decode_bech32_address`] — `addr1...` bech32 → raw address
//!   bytes (the form a CBOR output expects).

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use ed25519_dalek::{Signer, SigningKey, Verifier};

use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::PrivateKey;

/// TTL offset: latest slot + this many slots (~2 hours at 1 slot/sec).
const TTL_OFFSET: u64 = 7200;

/// Estimated transaction size for initial fee calculation (bytes).
///
/// Used to size the `min_fee_a * tx_size + min_fee_b` formula. Real
/// signed transactions vary slightly, so a 20% margin
/// ([`FEE_MARGIN_PERCENT`]) absorbs the difference without
/// rebuilding.
const ESTIMATED_TX_SIZE: u64 = 300;

/// Fee margin multiplier, in percent (120 = 20% extra).
const FEE_MARGIN_PERCENT: u64 = 120;

/// Blake2b-256 (32-byte output) used for the Cardano transaction
/// body hash.
type Blake2b256 = Blake2b<U32>;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Minimal RPC-shape-agnostic UTXO record.
///
/// Both Blockfrost (server) and Koios (browser) responses convert
/// to this shape before reaching [`build_signed_transfer`].
/// Multi-asset entries are filtered out by the caller; only the
/// lovelace component is needed for native ADA transfers.
#[derive(Clone, Debug)]
pub struct Utxo {
    /// Source transaction hash, hex-encoded.
    pub tx_hash: String,
    /// Output index inside the source transaction.
    pub output_index: u32,
    /// Value in lovelace (1 ADA = 1e6 lovelace).
    pub lovelace: u64,
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Build, sign, and CBOR-serialize a native ADA payment transaction.
///
/// Pipeline: greedy coin select → CBOR transaction body
/// (`{ 0: inputs, 1: outputs, 2: fee, 3: ttl }`) →
/// [`sign_tx_body`] (Blake2b-256 → Ed25519 → post-sign verify) →
/// re-encode the full transaction `[body, witness_set, true, null]`.
/// Returns the bytes ready for `submittx` / `tx/submit`.
///
/// `from_address_bytes` and `to_address_bytes` are the
/// bech32-decoded raw bytes — see [`decode_bech32_address`] for the
/// canonical conversion. `min_fee_a` and `min_fee_b` are the
/// network's protocol parameters (lovelace per byte and flat
/// component, respectively).
///
/// # Errors
/// - [`DontYeetWalletError::Validation`] if the UTXO set can't cover
///   `lovelace + estimated_fee` or the sender pubkey isn't 32 bytes.
/// - [`DontYeetWalletError::Crypto`] if signing fails.
#[expect(
    clippy::too_many_arguments,
    reason = "transaction-build helper takes all UTXO/fee/timing/key inputs; collapsing to a struct would obscure call sites"
)]
pub fn build_signed_transfer(
    utxos: &[Utxo],
    min_fee_a: u64,
    min_fee_b: u64,
    latest_slot: u64,
    from_address_bytes: &[u8],
    to_address_bytes: &[u8],
    sender_pubkey: &[u8],
    lovelace: u64,
    private_key: &PrivateKey,
) -> Result<Vec<u8>> {
    if sender_pubkey.len() != 32 {
        return Err(DontYeetWalletError::Validation(format!(
            "expected 32-byte Ed25519 pubkey, got {}",
            sender_pubkey.len()
        )));
    }

    let ttl = latest_slot.saturating_add(TTL_OFFSET);

    // Estimate fee with margin so we don't have to rebuild.
    let base_fee = min_fee_a
        .saturating_mul(ESTIMATED_TX_SIZE)
        .saturating_add(min_fee_b);
    let fee = base_fee.saturating_mul(FEE_MARGIN_PERCENT) / 100;

    let target = lovelace
        .checked_add(fee)
        .ok_or_else(|| DontYeetWalletError::Validation("amount + fee overflow".into()))?;

    let (selected, total_input) = select_utxos(utxos, target)?;
    let change = total_input - target;

    let body = encode_tx_body(
        &selected,
        to_address_bytes,
        lovelace,
        from_address_bytes,
        change,
        fee,
        ttl,
    );

    let signature = sign_tx_body(&body, private_key)?;

    Ok(encode_full_tx(&body, sender_pubkey, &signature))
}

/// Sign a CBOR-encoded transaction body and return the 64-byte
/// Ed25519 signature.
///
/// Computes Blake2b-256 of `body` (the canonical Cardano tx hash)
/// and signs the hash with Ed25519. Verifies the produced signature
/// against the signer's public key before returning, mirroring the
/// M.4.1 EVM safety pattern.
///
/// # Errors
/// Returns [`DontYeetWalletError::Crypto`] if `private_key` isn't 32 bytes
/// or post-sign verification fails.
pub fn sign_tx_body(body: &[u8], private_key: &PrivateKey) -> Result<[u8; 64]> {
    let key_bytes: [u8; 32] = private_key
        .as_bytes()
        .try_into()
        .map_err(|_| DontYeetWalletError::Crypto("Ed25519 key must be 32 bytes".into()))?;

    let signing_key = SigningKey::from_bytes(&key_bytes);
    let tx_hash = Blake2b256::digest(body);
    let signature = signing_key.sign(tx_hash.as_slice());

    // Post-sign verification: catches faulty signing hardware or
    // memory errors before the bad sig reaches the network.
    signing_key
        .verifying_key()
        .verify(tx_hash.as_slice(), &signature)
        .map_err(|e| DontYeetWalletError::Crypto(format!("post-sign verification failed: {e}")))?;

    Ok(signature.to_bytes())
}

/// Decode a Cardano bech32 address (`addr1...`, `addr_test1...`)
/// into its raw byte payload.
///
/// The raw bytes are what a CBOR output entry expects in field 0
/// (the address position). The HRP is discarded — both Blockfrost
/// and Koios accept either form on the wire.
///
/// # Errors
/// Returns [`DontYeetWalletError::Chain`] if the bech32 decode fails.
pub fn decode_bech32_address(address: &str) -> Result<Vec<u8>> {
    let (_hrp, data) =
        bech32::decode(address).map_err(|e| DontYeetWalletError::Chain(format!("bech32 decode: {e}")))?;
    Ok(data)
}

// ---------------------------------------------------------------------------
// Internal: coin selection
// ---------------------------------------------------------------------------

/// Greedy descending coin selection.
///
/// Picks the largest-value UTXO first, accumulating until the
/// running total covers `target_lovelace`. Returns the selected
/// UTXOs plus the total input value.
fn select_utxos(utxos: &[Utxo], target_lovelace: u64) -> Result<(Vec<&Utxo>, u64)> {
    let mut indexed: Vec<(u64, usize)> = utxos
        .iter()
        .enumerate()
        .map(|(i, u)| (u.lovelace, i))
        .collect();
    indexed.sort_unstable_by(|a, b| b.0.cmp(&a.0));

    let mut selected: Vec<&Utxo> = Vec::new();
    let mut total: u64 = 0;
    for (value, idx) in &indexed {
        selected.push(&utxos[*idx]);
        total = total.saturating_add(*value);
        if total >= target_lovelace {
            return Ok((selected, total));
        }
    }
    Err(DontYeetWalletError::Validation(format!(
        "insufficient funds: have {total} lovelace, need {target_lovelace}"
    )))
}

// ---------------------------------------------------------------------------
// Internal: CBOR encoding
// ---------------------------------------------------------------------------
//
// Manual CBOR construction for a Babbage-era payment transaction.
// Avoids pulling in a CBOR library dependency.

/// Encode the transaction body as a CBOR map.
///
/// ```text
/// { 0: inputs, 1: outputs, 2: fee, 3: ttl }
/// ```
fn encode_tx_body(
    inputs: &[&Utxo],
    to_addr: &[u8],
    to_amount: u64,
    change_addr: &[u8],
    change_amount: u64,
    fee: u64,
    ttl: u64,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256);

    // Map with 4 keys.
    cbor_map(&mut buf, 4);

    // Key 0: inputs — set of [tx_hash, output_index]
    cbor_uint(&mut buf, 0);
    cbor_array(&mut buf, inputs.len() as u64);
    for utxo in inputs {
        cbor_array(&mut buf, 2);
        let tx_hash = hex::decode(&utxo.tx_hash).unwrap_or_default();
        cbor_bytes(&mut buf, &tx_hash);
        cbor_uint(&mut buf, u64::from(utxo.output_index));
    }

    // Key 1: outputs
    let has_change = change_amount > 0;
    let n_outputs = if has_change { 2u64 } else { 1 };
    cbor_uint(&mut buf, 1);
    cbor_array(&mut buf, n_outputs);

    // Recipient output: [address_bytes, amount]
    cbor_array(&mut buf, 2);
    cbor_bytes(&mut buf, to_addr);
    cbor_uint(&mut buf, to_amount);

    // Change output (if any).
    if has_change {
        cbor_array(&mut buf, 2);
        cbor_bytes(&mut buf, change_addr);
        cbor_uint(&mut buf, change_amount);
    }

    // Key 2: fee
    cbor_uint(&mut buf, 2);
    cbor_uint(&mut buf, fee);

    // Key 3: TTL
    cbor_uint(&mut buf, 3);
    cbor_uint(&mut buf, ttl);

    buf
}

/// Encode the full transaction: `[body, witness_set, true, null]`.
fn encode_full_tx(body: &[u8], vkey: &[u8], signature: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(body.len() + 128);

    // Outer array(4)
    cbor_array(&mut buf, 4);

    // 0: transaction body (already CBOR-encoded)
    buf.extend_from_slice(body);

    // 1: witness set — { 0: [[vkey, signature]] }
    cbor_map(&mut buf, 1);
    cbor_uint(&mut buf, 0);
    cbor_array(&mut buf, 1);
    cbor_array(&mut buf, 2);
    cbor_bytes(&mut buf, vkey);
    cbor_bytes(&mut buf, signature);

    // 2: is_valid = true
    buf.push(0xF5);

    // 3: auxiliary_data = null
    buf.push(0xF6);

    buf
}

// ---- Low-level CBOR helpers ----

/// Encode a CBOR unsigned integer (major type 0).
fn cbor_uint(buf: &mut Vec<u8>, value: u64) {
    cbor_header(buf, 0, value);
}

/// Encode a CBOR byte string (major type 2).
fn cbor_bytes(buf: &mut Vec<u8>, data: &[u8]) {
    cbor_header(buf, 2, data.len() as u64);
    buf.extend_from_slice(data);
}

/// Encode a CBOR array header (major type 4).
fn cbor_array(buf: &mut Vec<u8>, len: u64) {
    cbor_header(buf, 4, len);
}

/// Encode a CBOR map header (major type 5).
fn cbor_map(buf: &mut Vec<u8>, len: u64) {
    cbor_header(buf, 5, len);
}

/// Write a CBOR header: major type (3 bits) + additional info.
fn cbor_header(buf: &mut Vec<u8>, major: u8, value: u64) {
    let mt = major << 5;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "branches below bound the cast to single-byte forms"
    )]
    if value < 24 {
        buf.push(mt | value as u8);
    } else if let Ok(b) = u8::try_from(value) {
        buf.push(mt | 0x18);
        buf.push(b);
    } else if let Ok(h) = u16::try_from(value) {
        buf.push(mt | 0x19);
        buf.extend_from_slice(&h.to_be_bytes());
    } else if let Ok(w) = u32::try_from(value) {
        buf.push(mt | 0x1A);
        buf.extend_from_slice(&w.to_be_bytes());
    } else {
        buf.push(mt | 0x1B);
        buf.extend_from_slice(&value.to_be_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_key() -> PrivateKey {
        PrivateKey::new(vec![1u8; 32])
    }

    fn fixture_pubkey() -> Vec<u8> {
        let key_bytes: [u8; 32] = [1u8; 32];
        let signing_key = SigningKey::from_bytes(&key_bytes);
        signing_key.verifying_key().to_bytes().to_vec()
    }

    fn make_utxo(tx_hash: &str, index: u32, lovelace: u64) -> Utxo {
        Utxo {
            tx_hash: tx_hash.into(),
            output_index: index,
            lovelace,
        }
    }

    #[test]
    fn cbor_uint_small() {
        let mut buf = Vec::new();
        cbor_uint(&mut buf, 0);
        assert_eq!(buf, vec![0x00]);

        buf.clear();
        cbor_uint(&mut buf, 23);
        assert_eq!(buf, vec![23]);
    }

    #[test]
    fn cbor_uint_one_byte() {
        let mut buf = Vec::new();
        cbor_uint(&mut buf, 24);
        assert_eq!(buf, vec![0x18, 24]);

        buf.clear();
        cbor_uint(&mut buf, 255);
        assert_eq!(buf, vec![0x18, 255]);
    }

    #[test]
    fn cbor_uint_two_bytes() {
        let mut buf = Vec::new();
        cbor_uint(&mut buf, 1000);
        assert_eq!(buf, vec![0x19, 0x03, 0xE8]);
    }

    #[test]
    fn cbor_uint_four_bytes() {
        let mut buf = Vec::new();
        cbor_uint(&mut buf, 1_000_000);
        assert_eq!(buf, vec![0x1A, 0x00, 0x0F, 0x42, 0x40]);
    }

    #[test]
    fn cbor_bytes_32() {
        let mut buf = Vec::new();
        cbor_bytes(&mut buf, &[0xAA; 32]);
        assert_eq!(buf[0], 0x58); // major type 2, additional 24
        assert_eq!(buf[1], 32);
        assert_eq!(buf.len(), 34);
    }

    #[test]
    fn cbor_array_header_4() {
        let mut buf = Vec::new();
        cbor_array(&mut buf, 4);
        assert_eq!(buf, vec![0x84]); // major type 4, value 4
    }

    #[test]
    fn cbor_map_header_1() {
        let mut buf = Vec::new();
        cbor_map(&mut buf, 1);
        assert_eq!(buf, vec![0xA1]); // major type 5, value 1
    }

    #[test]
    fn full_tx_starts_with_array_4() {
        let body_buf = {
            let mut b = Vec::new();
            cbor_map(&mut b, 0);
            b
        };
        let tx = encode_full_tx(&body_buf, &[0u8; 32], &[0u8; 64]);
        assert_eq!(tx[0], 0x84);
    }

    #[test]
    fn decode_bech32_address_round_trip() {
        let raw = vec![0x61u8; 29]; // header 0x61 + 28 bytes
        let addr_str =
            bech32::encode::<bech32::Bech32>(bech32::Hrp::parse("addr").expect("hrp"), &raw)
                .expect("encode");
        let decoded = decode_bech32_address(&addr_str).expect("decode");
        assert_eq!(decoded, raw);
    }

    #[test]
    fn decode_bech32_address_rejects_garbage() {
        assert!(decode_bech32_address("not-a-bech32-string").is_err());
    }

    #[test]
    fn select_utxos_picks_largest_first() {
        let utxos = vec![
            make_utxo("aa", 0, 5_000_000),
            make_utxo("bb", 0, 3_000_000),
            make_utxo("cc", 0, 10_000_000),
        ];
        let (selected, total) = select_utxos(&utxos, 4_000_000).expect("select");
        assert_eq!(total, 10_000_000);
        assert_eq!(selected.len(), 1);
    }

    #[test]
    fn select_utxos_insufficient() {
        let utxos = vec![make_utxo("aa", 0, 1_000_000)];
        assert!(matches!(
            select_utxos(&utxos, 5_000_000),
            Err(DontYeetWalletError::Validation(_))
        ));
    }

    #[test]
    fn sign_tx_body_returns_64_bytes() {
        let pk = fixture_key();
        let body = b"fake CBOR body";
        let sig = sign_tx_body(body, &pk).expect("sign");
        assert_eq!(sig.len(), 64);
    }

    #[test]
    fn sign_tx_body_is_deterministic() {
        let pk = fixture_key();
        let s1 = sign_tx_body(b"deterministic", &pk).expect("s1");
        let s2 = sign_tx_body(b"deterministic", &pk).expect("s2");
        assert_eq!(s1, s2);
    }

    #[test]
    fn sign_tx_body_rejects_short_key() {
        let pk = PrivateKey::new(vec![0u8; 16]);
        assert!(matches!(
            sign_tx_body(b"data", &pk),
            Err(DontYeetWalletError::Crypto(_))
        ));
    }

    #[test]
    fn build_signed_transfer_round_trip() {
        let pk = fixture_key();
        let pubkey = fixture_pubkey();
        let utxos = vec![make_utxo(&"aa".repeat(32), 0, 10_000_000)];
        let from_addr = vec![0x61u8; 29];
        let to_addr = vec![0x62u8; 29];

        let tx = build_signed_transfer(
            &utxos, 44,      // min_fee_a (canonical Babbage value)
            155_381, // min_fee_b
            100,     // latest slot
            &from_addr, &to_addr, &pubkey, 5_000_000, // 5 ADA
            &pk,
        )
        .expect("build");

        // Outermost CBOR is array(4): body, witness, is_valid, aux.
        assert_eq!(tx[0], 0x84);
    }

    #[test]
    fn build_signed_transfer_rejects_short_pubkey() {
        let pk = fixture_key();
        let utxos = vec![make_utxo(&"aa".repeat(32), 0, 10_000_000)];
        let from_addr = vec![0x61u8; 29];
        let to_addr = vec![0x62u8; 29];

        let err = build_signed_transfer(
            &utxos, 44, 155_381, 100, &from_addr, &to_addr, &[0u8; 16], // wrong length
            5_000_000, &pk,
        )
        .expect_err("must reject");
        assert!(matches!(err, DontYeetWalletError::Validation(_)));
    }

    #[test]
    fn build_signed_transfer_rejects_insufficient_funds() {
        let pk = fixture_key();
        let pubkey = fixture_pubkey();
        let utxos = vec![make_utxo(&"aa".repeat(32), 0, 100)];
        let from_addr = vec![0x61u8; 29];
        let to_addr = vec![0x62u8; 29];

        let err = build_signed_transfer(
            &utxos, 44, 155_381, 100, &from_addr, &to_addr, &pubkey, 5_000_000, &pk,
        )
        .expect_err("must reject");
        assert!(matches!(err, DontYeetWalletError::Validation(_)));
    }
}

// Rust guideline compliant 2026-02-21
