//! Pure-crypto Bitcoin/Litecoin P2WPKH transaction encoding and signing.
//!
//! Contains everything needed to turn a list of confirmed UTXOs plus a
//! recipient + amount + fee rate into a fully signed segwit (BIP-141 +
//! BIP-143) transaction blob ready for broadcast via mempool.space's
//! `POST /tx`. No network I/O, no side effects — both the server-side
//! [`crate::transfer`] pipeline and the in-browser [`crate::wasm::send`]
//! entry point fetch UTXOs and fee rate via their own HTTP clients,
//! then call [`build_signed_transfer`].
//!
//! Lives outside the `feature = "rpc"` gate so browser consumers
//! (`default-features = false`) can sign transactions without pulling
//! in `reqwest`, `tokio`, or any other server-only dependency. Phase
//! M.4.2 mirrors the M.4.1 EVM pattern: extract the signing core into
//! its own module, then have both the rpc-feature glue and the wasm
//! glue delegate to it.
//!
//! ## Layout
//!
//! - [`Utxo`] — minimal RPC-shape-agnostic UTXO record. Both Mempool.space
//!   (server) and litecoinspace.org (browser) responses convert to this
//!   shape before reaching [`build_signed_transfer`].
//! - [`SignedTransfer`] — output bundle: raw tx bytes plus fee/change
//!   in sats for telemetry.
//! - [`build_signed_transfer`] — full pipeline: greedy coin selection
//!   → BIP-143 preimage per input → ECDSA → DER + `SIGHASH_ALL` →
//!   serialize the segwit body.
//! - [`decode_p2wpkh_address`] / [`p2wpkh_script_pubkey`] — bech32 +
//!   script helpers shared with both call sites.
//! - [`sign_p2wpkh_input`] — single-input sighash → ECDSA primitive,
//!   re-used by [`crate::tx::BtcTransactionSigner`] so the
//!   `ChainPlugin::signer()` path goes through the same code as the
//!   transfer pipeline.
//!
//! ## Why this file is over the 600-line CLAUDE.md guideline
//!
//! Bitcoin segwit signing has irreducible complexity: BIP-141 wire
//! format, BIP-143 sighash preimage, varint encoding, coin selection,
//! dust handling, witness assembly, and DER signing. The protocol
//! logic lands at ~525 LOC; thorough unit tests add ~240 more.
//!
//! Splitting was considered (one PR per concern: coin select / sighash
//! / serialize / witness) but rejected because:
//!
//! 1. **Single-concern.** Everything here exists to build one
//!    P2WPKH transaction; the section markers below act as the
//!    seams without forcing the reader to chase imports.
//! 2. **Security crate.** This module signs transactions — every
//!    refactor adds review burden and regression risk. The benefit
//!    of three sub-modules is cosmetic.
//! 3. **No dependency shrink available.** Pulling in the `bitcoin`
//!    crate would shed lines but inflates trust surface for the
//!    signing path, against CLAUDE.md's "minimize dependencies in
//!    the signing/crypto crate" rule.
//!
//! Accept the size as a deliberate exception. If this file grows
//! past 1000 lines (the hard "must split" threshold) or gains a
//! second concern, revisit.

use k256::ecdsa::{Signature, SigningKey, signature::hazmat::PrehashSigner};
use sha2::{Digest, Sha256};

use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::PrivateKey;

use crate::keys::hash160;

/// Transaction version. `SegWit` v0 transactions use `nVersion = 2`.
const TX_VERSION: u32 = 2;

/// Default `nSequence` for every input — opt-in RBF disabled.
const SEQUENCE: u32 = 0xFFFF_FFFF;

/// Estimated vsize per P2WPKH input, used during coin selection.
const INPUT_VSIZE: u64 = 68;

/// Estimated vsize per P2WPKH output, used during coin selection.
const OUTPUT_VSIZE: u64 = 31;

/// Base transaction vsize overhead: version + marker + flag + locktime
/// + counters.
const BASE_VSIZE: u64 = 11;

/// Minimum sats a change output must hold to be emitted.
///
/// Standard Bitcoin dust threshold for P2PKH; we apply it to P2WPKH
/// too because the difference (~294 sats) isn't worth surfacing as a
/// separate constant and 546 is what the existing tests pin.
const CHANGE_DUST_THRESHOLD: u64 = 546;

/// `SIGHASH_ALL` sighash type byte appended after each DER signature.
const SIGHASH_ALL: u8 = 0x01;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Minimal RPC-shape-agnostic UTXO record.
///
/// Both Mempool.space (`/api/address/{addr}/utxo`) and
/// litecoinspace.org responses convert to this shape before reaching
/// [`build_signed_transfer`]. Callers are responsible for filtering to
/// confirmed UTXOs only.
#[derive(Clone, Debug)]
pub struct Utxo {
    /// Transaction id in display (big-endian) hex order. The internal
    /// little-endian byte order needed for serialization is computed
    /// inside this module.
    pub txid: String,
    /// Output index inside the source transaction.
    pub vout: u32,
    /// Value in the chain's native sub-unit (sats / litoshis).
    pub value_sats: u64,
}

/// Output of a successful build-and-sign run.
#[derive(Debug)]
pub struct SignedTransfer {
    /// Fully serialized signed segwit transaction, ready for
    /// `POST /tx` with hex encoding.
    pub raw_tx: Vec<u8>,
    /// Total fee paid in sats (selected total − amount − change).
    pub fee_sats: u64,
    /// Change output value in sats. `0` if change was below dust and
    /// got rolled into the fee.
    pub change_sats: u64,
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Build and sign a P2WPKH segwit native transfer.
///
/// Pipeline: greedy descending coin selection → compute BIP-143
/// shared hashes (`hashPrevouts`, `hashSequence`, `hashOutputs`) →
/// per-input preimage → secp256k1 ECDSA + DER + `SIGHASH_ALL` →
/// serialize the segwit body with one witness stack per input.
///
/// `sender_pubkey` must be the 33-byte compressed secp256k1 public
/// key matching `private_key`; it ends up in every input's witness
/// stack and its `hash160` becomes the change output's witness
/// program. Callers pre-fetch UTXOs and fee rate.
///
/// # Errors
/// - [`DontYeetWalletError::Validation`] if `sender_pubkey` isn't 33 bytes,
///   `confirmed_utxos` is empty, total selected funds can't cover
///   `amount_sats + fee`, or any UTXO `txid` isn't valid 32-byte hex.
/// - [`DontYeetWalletError::Crypto`] if the private key is rejected by
///   secp256k1 or signing fails.
pub fn build_signed_transfer(
    confirmed_utxos: &[Utxo],
    fee_rate_sat_per_vbyte: u64,
    sender_pubkey: &[u8],
    recipient_witness_program: &[u8; 20],
    private_key: &PrivateKey,
    amount_sats: u64,
) -> Result<SignedTransfer> {
    if sender_pubkey.len() != 33 {
        return Err(DontYeetWalletError::Validation(format!(
            "expected 33-byte compressed pubkey, got {}",
            sender_pubkey.len()
        )));
    }
    if confirmed_utxos.is_empty() {
        return Err(DontYeetWalletError::Validation(
            "no confirmed UTXOs available".into(),
        ));
    }

    let sender_hash = hash160(sender_pubkey);

    // 1. Coin selection (greedy descending). 2 outputs accounts for the
    //    worst case (recipient + change); change-below-dust is handled
    //    after by rolling the leftover into the fee.
    let n_outputs_for_fee = 2u64;
    let (selected_indices, total_input, fee) = select_utxos(
        confirmed_utxos,
        amount_sats,
        fee_rate_sat_per_vbyte,
        n_outputs_for_fee,
    )?;

    let raw_change = total_input
        .checked_sub(amount_sats)
        .and_then(|v| v.checked_sub(fee))
        .ok_or_else(|| DontYeetWalletError::Validation("fee calculation underflow".into()))?;

    // 2. Decode each selected UTXO's txid into the 32-byte internal
    //    (little-endian) form once up front. Failing here means the
    //    upstream RPC fed us malformed data — surface that as a
    //    Validation error instead of silently signing zeroed bytes.
    let selected: Vec<DecodedUtxo> = selected_indices
        .iter()
        .map(|&i| DecodedUtxo::from_utxo(&confirmed_utxos[i]))
        .collect::<Result<_>>()?;

    // 3. Build the output set. Drop the change output if it's below
    //    dust — those sats become an extra fee tip.
    let mut outputs: Vec<(u64, Vec<u8>)> = Vec::with_capacity(2);
    outputs.push((amount_sats, p2wpkh_script_pubkey(recipient_witness_program)));
    let emit_change = raw_change > CHANGE_DUST_THRESHOLD;
    if emit_change {
        outputs.push((raw_change, p2wpkh_script_pubkey(&sender_hash)));
    }

    // 4. BIP-143 shared hashes (computed once for all inputs).
    let hash_prevouts = sha256d(&serialize_prevouts(&selected));
    let hash_sequence = sha256d(&serialize_sequences(selected.len()));
    let hash_outputs = sha256d(&serialize_outputs(&outputs));

    // 5. Sign each input.
    let mut witnesses: Vec<Vec<u8>> = Vec::with_capacity(selected.len());
    for utxo in &selected {
        let preimage = build_bip143_preimage(
            utxo,
            &sender_hash,
            &hash_prevouts,
            &hash_sequence,
            &hash_outputs,
        );
        witnesses.push(sign_p2wpkh_input(&preimage, private_key)?);
    }

    let raw_tx = serialize_segwit_tx(&selected, &outputs, &witnesses, sender_pubkey);

    // If change went to dust, the actual paid fee is total_input − amount.
    let actual_fee = if emit_change {
        fee
    } else {
        total_input - amount_sats
    };
    let actual_change = if emit_change { raw_change } else { 0 };

    Ok(SignedTransfer {
        raw_tx,
        fee_sats: actual_fee,
        change_sats: actual_change,
    })
}

/// Sign one BIP-143 P2WPKH preimage and return the witness signature.
///
/// Double-SHA256s the preimage, signs with secp256k1 ECDSA, returns
/// the DER-encoded signature with the `SIGHASH_ALL` sighash type byte
/// appended. Used by both [`build_signed_transfer`] (per-input loop)
/// and the [`crate::tx::BtcTransactionSigner`] trait impl that
/// satisfies `ChainPlugin::signer()`.
///
/// # Errors
/// Returns [`DontYeetWalletError::Crypto`] if `private_key` is rejected by
/// secp256k1 or signing fails.
pub fn sign_p2wpkh_input(preimage: &[u8], private_key: &PrivateKey) -> Result<Vec<u8>> {
    // Standard Bitcoin sighash: SHA256(SHA256(preimage)).
    let h1 = Sha256::digest(preimage);
    let sighash = Sha256::digest(h1);

    let signing_key = SigningKey::from_slice(private_key.as_bytes())
        .map_err(|e| DontYeetWalletError::Crypto(format!("invalid signing key: {e}")))?;

    let signature: Signature = signing_key
        .sign_prehash(sighash.as_slice())
        .map_err(|e| DontYeetWalletError::Crypto(format!("ECDSA sign failed: {e}")))?;

    let der = signature.to_der();
    let mut out = der.as_bytes().to_vec();
    out.push(SIGHASH_ALL);
    Ok(out)
}

/// Decode a bech32 P2WPKH address into its 20-byte witness program.
///
/// Accepts segwit v0 addresses for any HRP (`bc1...`, `tb1...`,
/// `ltc1...`). The HRP is not checked here — the caller is expected
/// to have already validated the address via
/// [`crate::keys::BtcAddressEncoder::validate`] (server) or the UI's
/// chain-aware send form.
///
/// # Errors
/// Returns [`DontYeetWalletError::Chain`] if the address isn't a valid
/// bech32 segwit address or the witness program isn't exactly 20
/// bytes (i.e. not a P2WPKH).
pub fn decode_p2wpkh_address(address: &str) -> Result<[u8; 20]> {
    let (_hrp, _version, witness_program) = bech32::segwit::decode(address)
        .map_err(|e| DontYeetWalletError::Chain(format!("bech32 decode: {e}")))?;
    if witness_program.len() != 20 {
        return Err(DontYeetWalletError::Chain(format!(
            "expected 20-byte witness program, got {}",
            witness_program.len()
        )));
    }
    let mut hash = [0u8; 20];
    hash.copy_from_slice(&witness_program);
    Ok(hash)
}

/// Build a P2WPKH `scriptPubKey`: `OP_0 OP_PUSH20 <20-byte-hash>`.
#[must_use]
pub fn p2wpkh_script_pubkey(pubkey_hash: &[u8; 20]) -> Vec<u8> {
    let mut script = Vec::with_capacity(22);
    script.push(0x00); // OP_0 (witness version)
    script.push(0x14); // OP_PUSH20
    script.extend_from_slice(pubkey_hash);
    script
}

// ---------------------------------------------------------------------------
// Internal: decoded UTXO (txid pre-converted to internal byte order)
// ---------------------------------------------------------------------------

/// A [`Utxo`] with `txid` already decoded and reversed to the internal
/// little-endian byte order used by serialized transactions.
///
/// Done up front so a malformed hex string surfaces as a Validation
/// error rather than silently producing zeroed bytes downstream.
struct DecodedUtxo {
    txid_internal: [u8; 32],
    vout: u32,
    value_sats: u64,
}

impl DecodedUtxo {
    fn from_utxo(utxo: &Utxo) -> Result<Self> {
        let mut bytes = hex::decode(&utxo.txid).map_err(|e| {
            DontYeetWalletError::Validation(format!("invalid txid hex '{}': {e}", utxo.txid))
        })?;
        if bytes.len() != 32 {
            return Err(DontYeetWalletError::Validation(format!(
                "txid must be 32 bytes, got {} for '{}'",
                bytes.len(),
                utxo.txid
            )));
        }
        bytes.reverse();
        let mut txid_internal = [0u8; 32];
        txid_internal.copy_from_slice(&bytes);
        Ok(Self {
            txid_internal,
            vout: utxo.vout,
            value_sats: utxo.value_sats,
        })
    }
}

// ---------------------------------------------------------------------------
// Coin selection
// ---------------------------------------------------------------------------

/// Greedy descending coin selection with fee estimation.
///
/// Returns `(selected_indices, total_input, fee)`. Picks the largest
/// UTXO first, accumulating until the running total covers `target +
/// dynamic fee`. Same algorithm the server has been using since the
/// initial Bitcoin transfer pipeline.
fn select_utxos(
    utxos: &[Utxo],
    target_sats: u64,
    fee_rate_sat_per_vbyte: u64,
    n_outputs: u64,
) -> Result<(Vec<usize>, u64, u64)> {
    let mut indexed: Vec<(u64, usize)> = utxos
        .iter()
        .enumerate()
        .map(|(i, u)| (u.value_sats, i))
        .collect();
    indexed.sort_unstable_by(|a, b| b.0.cmp(&a.0));

    let mut selected: Vec<usize> = Vec::new();
    let mut total: u64 = 0;

    for (value, idx) in &indexed {
        selected.push(*idx);
        total = total.saturating_add(*value);

        let n_inputs = selected.len() as u64;
        let vsize = BASE_VSIZE + INPUT_VSIZE * n_inputs + OUTPUT_VSIZE * n_outputs;
        let fee = vsize.saturating_mul(fee_rate_sat_per_vbyte);

        if let Some(remaining) = total.checked_sub(target_sats)
            && remaining >= fee
        {
            return Ok((selected, total, fee));
        }
    }

    Err(DontYeetWalletError::Validation(format!(
        "insufficient funds: have {total} sats, need {target_sats} + fees"
    )))
}

// ---------------------------------------------------------------------------
// BIP-143 sighash
// ---------------------------------------------------------------------------

/// Build the BIP-143 sighash preimage for one P2WPKH input.
///
/// The preimage is exactly what gets double-SHA256-hashed before
/// signing. See BIP-143 for the field layout.
fn build_bip143_preimage(
    utxo: &DecodedUtxo,
    sender_hash: &[u8; 20],
    hash_prevouts: &[u8; 32],
    hash_sequence: &[u8; 32],
    hash_outputs: &[u8; 32],
) -> Vec<u8> {
    let mut preimage = Vec::with_capacity(182);

    // 1. nVersion
    preimage.extend_from_slice(&TX_VERSION.to_le_bytes());
    // 2. hashPrevouts
    preimage.extend_from_slice(hash_prevouts);
    // 3. hashSequence
    preimage.extend_from_slice(hash_sequence);
    // 4. outpoint (txid little-endian + vout)
    preimage.extend_from_slice(&utxo.txid_internal);
    preimage.extend_from_slice(&utxo.vout.to_le_bytes());
    // 5. scriptCode for P2WPKH:
    //    OP_DUP OP_HASH160 OP_PUSH20 <hash> OP_EQUALVERIFY OP_CHECKSIG
    preimage.push(0x19); // length = 25
    preimage.push(0x76); // OP_DUP
    preimage.push(0xa9); // OP_HASH160
    preimage.push(0x14); // OP_PUSH20
    preimage.extend_from_slice(sender_hash);
    preimage.push(0x88); // OP_EQUALVERIFY
    preimage.push(0xac); // OP_CHECKSIG
    // 6. value
    preimage.extend_from_slice(&utxo.value_sats.to_le_bytes());
    // 7. nSequence
    preimage.extend_from_slice(&SEQUENCE.to_le_bytes());
    // 8. hashOutputs
    preimage.extend_from_slice(hash_outputs);
    // 9. nLockTime
    preimage.extend_from_slice(&0u32.to_le_bytes());
    // 10. sighash type
    preimage.extend_from_slice(&u32::from(SIGHASH_ALL).to_le_bytes());

    preimage
}

// ---------------------------------------------------------------------------
// Transaction body serialization
// ---------------------------------------------------------------------------

/// Serialize the full segwit transaction for broadcasting.
fn serialize_segwit_tx(
    inputs: &[DecodedUtxo],
    outputs: &[(u64, Vec<u8>)],
    witnesses: &[Vec<u8>],
    sender_pubkey: &[u8],
) -> Vec<u8> {
    let mut tx = Vec::with_capacity(256);

    // Version
    tx.extend_from_slice(&TX_VERSION.to_le_bytes());
    // SegWit marker + flag
    tx.push(0x00);
    tx.push(0x01);

    // Input count + inputs
    write_varint(&mut tx, inputs.len() as u64);
    for utxo in inputs {
        tx.extend_from_slice(&utxo.txid_internal);
        tx.extend_from_slice(&utxo.vout.to_le_bytes());
        tx.push(0x00); // empty scriptSig (segwit)
        tx.extend_from_slice(&SEQUENCE.to_le_bytes());
    }

    // Output count + outputs
    write_varint(&mut tx, outputs.len() as u64);
    for (value, script) in outputs {
        tx.extend_from_slice(&value.to_le_bytes());
        write_varint(&mut tx, script.len() as u64);
        tx.extend_from_slice(script);
    }

    // Witnesses (one stack per input: 2 items — signature, pubkey).
    for sig in witnesses {
        tx.push(0x02);
        write_varint(&mut tx, sig.len() as u64);
        tx.extend_from_slice(sig);
        write_varint(&mut tx, sender_pubkey.len() as u64);
        tx.extend_from_slice(sender_pubkey);
    }

    // Locktime
    tx.extend_from_slice(&0u32.to_le_bytes());

    tx
}

/// Serialize all input prevouts into the buffer that gets hashed for
/// `hashPrevouts`.
fn serialize_prevouts(utxos: &[DecodedUtxo]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(utxos.len() * 36);
    for utxo in utxos {
        buf.extend_from_slice(&utxo.txid_internal);
        buf.extend_from_slice(&utxo.vout.to_le_bytes());
    }
    buf
}

/// Serialize all input `nSequence` values for `hashSequence`.
fn serialize_sequences(count: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(count * 4);
    for _ in 0..count {
        buf.extend_from_slice(&SEQUENCE.to_le_bytes());
    }
    buf
}

/// Serialize all outputs for `hashOutputs`.
fn serialize_outputs(outputs: &[(u64, Vec<u8>)]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(outputs.len() * 34);
    for (value, script) in outputs {
        buf.extend_from_slice(&value.to_le_bytes());
        write_varint(&mut buf, script.len() as u64);
        buf.extend_from_slice(script);
    }
    buf
}

// ---------------------------------------------------------------------------
// Encoding primitives
// ---------------------------------------------------------------------------

/// `CompactSize` (varint) encoding used by the Bitcoin wire format.
fn write_varint(buf: &mut Vec<u8>, n: u64) {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "branch above ensures the cast fits"
    )]
    if n < 0xFD {
        buf.push(n as u8);
    } else if n <= 0xFFFF {
        buf.push(0xFD);
        buf.extend_from_slice(&(n as u16).to_le_bytes());
    } else if n <= 0xFFFF_FFFF {
        buf.push(0xFE);
        buf.extend_from_slice(&(n as u32).to_le_bytes());
    } else {
        buf.push(0xFF);
        buf.extend_from_slice(&n.to_le_bytes());
    }
}

/// Double-SHA256 hash, the standard Bitcoin/segwit hash primitive.
fn sha256d(data: &[u8]) -> [u8; 32] {
    let h1 = Sha256::digest(data);
    let h2 = Sha256::digest(h1);
    let mut out = [0u8; 32];
    out.copy_from_slice(&h2);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal fixture: one UTXO with the all-ones placeholder txid.
    fn one_utxo(value_sats: u64) -> Utxo {
        Utxo {
            txid: "11".repeat(32),
            vout: 0,
            value_sats,
        }
    }

    #[test]
    fn p2wpkh_script_is_22_bytes() {
        let hash = [0xAA; 20];
        let script = p2wpkh_script_pubkey(&hash);
        assert_eq!(script.len(), 22);
        assert_eq!(script[0], 0x00); // OP_0
        assert_eq!(script[1], 0x14); // OP_PUSH20
    }

    #[test]
    fn varint_small_medium_large() {
        let mut buf = Vec::new();
        write_varint(&mut buf, 42);
        assert_eq!(buf, vec![42]);

        let mut buf = Vec::new();
        write_varint(&mut buf, 300);
        assert_eq!(buf, vec![0xFD, 0x2C, 0x01]);

        let mut buf = Vec::new();
        write_varint(&mut buf, 70_000);
        assert_eq!(buf, vec![0xFE, 0x70, 0x11, 0x01, 0x00]);
    }

    #[test]
    fn sha256d_known_vector() {
        let result = sha256d(b"");
        let expected =
            hex::decode("5df6e0e2761359d30a8275058e299fcc0381534545f55cf43e41983f5d4c9456")
                .expect("hex");
        assert_eq!(result.as_slice(), expected.as_slice());
    }

    #[test]
    fn bip143_preimage_length_is_182() {
        let utxo = DecodedUtxo {
            txid_internal: [0u8; 32],
            vout: 0,
            value_sats: 50_000,
        };
        let preimage = build_bip143_preimage(&utxo, &[0u8; 20], &[0u8; 32], &[0u8; 32], &[0u8; 32]);
        // 4 + 32 + 32 + 36 + 26 + 8 + 4 + 32 + 4 + 4 = 182.
        assert_eq!(preimage.len(), 182);
    }

    #[test]
    fn select_utxos_picks_largest_first() {
        let utxos = vec![
            Utxo {
                txid: "aa".repeat(32),
                vout: 0,
                value_sats: 100_000,
            },
            Utxo {
                txid: "bb".repeat(32),
                vout: 1,
                value_sats: 50_000,
            },
        ];
        let (selected, total, fee) = select_utxos(&utxos, 40_000, 5, 2).expect("select");
        assert_eq!(selected, vec![0]); // 100k UTXO covers 40k + fee alone
        assert_eq!(total, 100_000);
        assert!(fee > 0);
    }

    #[test]
    fn select_utxos_insufficient_returns_validation_error() {
        let utxos = vec![one_utxo(100)];
        let err = select_utxos(&utxos, 50_000, 5, 2).expect_err("must fail");
        assert!(
            matches!(err, DontYeetWalletError::Validation(_)),
            "expected Validation, got {err:?}"
        );
    }

    #[test]
    fn decoded_utxo_rejects_non_hex_txid() {
        let utxo = Utxo {
            txid: "not-hex".into(),
            vout: 0,
            value_sats: 1_000,
        };
        assert!(matches!(
            DecodedUtxo::from_utxo(&utxo),
            Err(DontYeetWalletError::Validation(_))
        ));
    }

    #[test]
    fn decoded_utxo_rejects_wrong_length_txid() {
        let utxo = Utxo {
            txid: "ab".into(), // 1 byte, not 32
            vout: 0,
            value_sats: 1_000,
        };
        assert!(matches!(
            DecodedUtxo::from_utxo(&utxo),
            Err(DontYeetWalletError::Validation(_))
        ));
    }

    #[test]
    fn decoded_utxo_reverses_txid() {
        let utxo = Utxo {
            txid: "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20".into(),
            vout: 7,
            value_sats: 12_345,
        };
        let decoded = DecodedUtxo::from_utxo(&utxo).expect("decode");
        // Last byte of display order becomes first byte of internal.
        assert_eq!(decoded.txid_internal[0], 0x20);
        assert_eq!(decoded.txid_internal[31], 0x01);
        assert_eq!(decoded.vout, 7);
    }

    #[test]
    fn decode_p2wpkh_address_round_trip() {
        // bc1q known-good address derived elsewhere in this crate's tests.
        let addr = "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4";
        let program = decode_p2wpkh_address(addr).expect("decode");
        assert_eq!(program.len(), 20);
    }

    #[test]
    fn decode_p2wpkh_rejects_garbage() {
        assert!(decode_p2wpkh_address("not_an_address").is_err());
    }

    #[test]
    fn sign_p2wpkh_input_returns_der_with_sighash_byte() {
        let key_bytes =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .expect("hex");
        let pk = PrivateKey::new(key_bytes);
        let sig = sign_p2wpkh_input(b"fake preimage", &pk).expect("sign");
        // DER signature is 70-72 bytes plus the 1-byte sighash type.
        assert!(sig.len() >= 68 && sig.len() <= 73);
        assert_eq!(*sig.last().expect("non-empty"), SIGHASH_ALL);
    }

    #[test]
    fn sign_p2wpkh_input_rejects_zero_key() {
        let pk = PrivateKey::new(vec![0u8; 32]);
        assert!(matches!(
            sign_p2wpkh_input(b"data", &pk),
            Err(DontYeetWalletError::Crypto(_))
        ));
    }

    #[test]
    fn build_signed_transfer_rejects_short_pubkey() {
        let pk = PrivateKey::new(vec![1u8; 32]);
        let utxos = vec![one_utxo(100_000)];
        let recipient = [0xCC; 20];
        let err = build_signed_transfer(&utxos, 5, &[0u8; 32], &recipient, &pk, 10_000)
            .expect_err("must reject 32-byte pubkey");
        assert!(matches!(err, DontYeetWalletError::Validation(_)));
    }

    #[test]
    fn build_signed_transfer_rejects_empty_utxos() {
        let pk = PrivateKey::new(vec![1u8; 32]);
        let pub_compressed = vec![0u8; 33]; // contents don't matter for length check
        let recipient = [0xCC; 20];
        let err = build_signed_transfer(&[], 5, &pub_compressed, &recipient, &pk, 10_000)
            .expect_err("must reject");
        assert!(matches!(err, DontYeetWalletError::Validation(_)));
    }

    #[test]
    fn build_signed_transfer_end_to_end_one_input_one_output() {
        // Use private key = 1 so we can reuse the well-known compressed
        // pubkey already exercised by `keys::tests`.
        let key_bytes =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .expect("hex");
        let pk = PrivateKey::new(key_bytes);
        let pub_compressed = crate::keys::compressed_pubkey(&pk).expect("pubkey");

        // Coin-selection assumes 2 outputs: vsize = 11 + 68 + 31*2 = 141,
        // fee at 5 sat/vB = 705 sats. Pick a UTXO that just covers
        // amount + 2-output fee, leaving change strictly below dust so
        // the change output gets rolled into the fee (single-output tx).
        let utxos = vec![Utxo {
            txid: "aa".repeat(32),
            vout: 0,
            value_sats: 11_000,
        }];
        let recipient = [0xCC; 20];

        let result = build_signed_transfer(&utxos, 5, &pub_compressed, &recipient, &pk, 10_000)
            .expect("build");

        // Marker + flag are at offsets 4 and 5 of every segwit tx.
        assert_eq!(result.raw_tx[4], 0x00);
        assert_eq!(result.raw_tx[5], 0x01);
        // raw_change = 11000 - 10000 - 705 = 295 ≤ 546 dust → no change
        // output, fee absorbs the leftover (= 1000 sats total).
        assert_eq!(result.change_sats, 0);
        assert_eq!(result.fee_sats, 1_000);
    }

    #[test]
    fn build_signed_transfer_emits_change_when_above_dust() {
        let key_bytes =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .expect("hex");
        let pk = PrivateKey::new(key_bytes);
        let pub_compressed = crate::keys::compressed_pubkey(&pk).expect("pubkey");

        let utxos = vec![Utxo {
            txid: "ab".repeat(32),
            vout: 0,
            value_sats: 100_000,
        }];
        let recipient = [0xDD; 20];

        let result = build_signed_transfer(&utxos, 5, &pub_compressed, &recipient, &pk, 10_000)
            .expect("build");

        assert!(result.change_sats > CHANGE_DUST_THRESHOLD);
        assert!(result.fee_sats > 0);
    }
}

// Rust guideline compliant 2026-02-21
