//! Pure-crypto XRP Ledger Payment encoding and signing.
//!
//! Builds, signs, and serializes a native-XRP `Payment` transaction
//! in XRPL's binary format without any I/O. Both server-side
//! [`crate::transfer`] and the in-browser [`crate::wasm::send`]
//! pre-fetch account sequence and current ledger index, then call
//! [`build_signed_payment`] to produce the `tx_blob` that the
//! `submit` JSON-RPC method expects.
//!
//! Lives outside the `feature = "rpc"` gate so browser consumers
//! (`default-features = false`) can sign without pulling in
//! `reqwest`, `tokio`, or any other server-only dependency. Phase
//! M.4.4 mirrors the M.4.1 / M.4.2 / M.4.3 split: extract the
//! protocol logic into a shared module, then have both the
//! rpc-feature glue and the wasm glue delegate to it.
//!
//! ## Layout
//!
//! - [`PaymentParams`] — caller-supplied inputs: amount, fee,
//!   sequence, ledger window, sender + recipient account IDs.
//! - [`build_signed_payment`] — full pipeline: serialize unsigned
//!   form → SHA-512 Half over `STX\0 || unsigned` → secp256k1
//!   ECDSA → DER → re-serialize with `TxnSignature` field. Returns
//!   the raw blob ready for `hex::encode` + `submit`.
//! - [`address_to_account_id`] — XRPL custom-Base58 address
//!   (`r...`) → 20-byte account ID, including checksum verification.
//! - [`sign_unsigned_payload`] — single-shot signing primitive
//!   (XRPL prefix + SHA-512 Half + ECDSA + DER) re-used by
//!   [`crate::tx::XrpTransactionSigner`] so the
//!   `ChainPlugin::signer()` path goes through identical code.

use k256::ecdsa::{
    Signature, SigningKey, signature::hazmat::PrehashSigner, signature::hazmat::PrehashVerifier,
};
use sha2::{Digest, Sha256, Sha512};

use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::PrivateKey;

use crate::keys::xrp_base58_decode;

/// XRP single-sign hash prefix (`STX\0` = `0x53545800`).
const HASH_PREFIX_TX_SIGN: [u8; 4] = [0x53, 0x54, 0x58, 0x00];

// ---- XRPL binary type / field codes ----

const TYPE_UINT16: u8 = 1;
const TYPE_UINT32: u8 = 2;
const TYPE_AMOUNT: u8 = 6;
const TYPE_BLOB: u8 = 7;
const TYPE_ACCOUNT: u8 = 8;

const FIELD_TRANSACTION_TYPE: u8 = 2;
const FIELD_FLAGS: u8 = 2;
const FIELD_SEQUENCE: u8 = 4;
const FIELD_LAST_LEDGER_SEQ: u8 = 27;
const FIELD_AMOUNT: u8 = 1;
const FIELD_FEE: u8 = 8;
const FIELD_SIGNING_PUB_KEY: u8 = 3;
const FIELD_TXN_SIGNATURE: u8 = 4;
const FIELD_ACCOUNT: u8 = 1;
const FIELD_DESTINATION: u8 = 3;

/// XRP `Payment` transaction-type value.
const TX_TYPE_PAYMENT: u16 = 0;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Caller-supplied inputs for a native-XRP Payment transaction.
///
/// `signing_pub_key` must be the 33-byte compressed secp256k1 public
/// key matching the signing private key — XRP nodes verify by
/// recovering the account ID from this field rather than from the
/// signature alone, so it must round-trip to the sender's account ID.
#[derive(Debug, Clone)]
pub struct PaymentParams {
    /// 20-byte account ID of the sender.
    pub from_account_id: [u8; 20],
    /// 20-byte account ID of the recipient.
    pub to_account_id: [u8; 20],
    /// Amount to transfer in drops (1 XRP = 1e6 drops).
    pub drops: u64,
    /// Fee to pay in drops. XRP base fee is 12 drops on mainnet.
    pub fee_drops: u64,
    /// Sender's account sequence number.
    pub sequence: u64,
    /// Last ledger index this transaction is valid for. Use
    /// `current_ledger + 20` as a default ~80 second validity window.
    pub last_ledger_seq: u64,
    /// 33-byte compressed secp256k1 public key of the signer.
    pub signing_pub_key: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Build and sign a native-XRP Payment transaction.
///
/// Pipeline: serialize the unsigned form (no `TxnSignature`) →
/// prefix `STX\0` → SHA-512 Half → secp256k1 ECDSA → DER → re-serialize
/// with the `TxnSignature` field included. Returns the raw blob ready
/// for `hex::encode` + the `submit` JSON-RPC method.
///
/// # Errors
/// - [`DontYeetWalletError::Validation`] if `signing_pub_key` isn't 33 bytes.
/// - [`DontYeetWalletError::Crypto`] if `private_key` is rejected by
///   secp256k1, signing fails, or the post-sign verification step
///   reports the produced signature does not validate against the
///   signer's public key.
pub fn build_signed_payment(params: &PaymentParams, private_key: &PrivateKey) -> Result<Vec<u8>> {
    if params.signing_pub_key.len() != 33 {
        return Err(DontYeetWalletError::Validation(format!(
            "expected 33-byte compressed pubkey, got {}",
            params.signing_pub_key.len()
        )));
    }

    let unsigned = serialize_payment(params, None);
    let signature = sign_unsigned_payload(&unsigned, private_key)?;
    Ok(serialize_payment(params, Some(&signature)))
}

/// Sign an XRPL unsigned-payload blob and return the DER signature.
///
/// Prepends the single-sign hash prefix `STX\0`, computes SHA-512
/// Half (first 32 bytes of SHA-512), signs with secp256k1 ECDSA, and
/// returns the DER-encoded signature ready to splice into a Payment
/// as `TxnSignature`. Verifies the produced signature against the
/// signer's public key before returning — catches faulty signing
/// hardware or memory errors before the bad sig reaches the network.
///
/// # Errors
/// Returns [`DontYeetWalletError::Crypto`] if `private_key` is rejected by
/// secp256k1, signing fails, or post-sign verification fails.
pub fn sign_unsigned_payload(unsigned: &[u8], private_key: &PrivateKey) -> Result<Vec<u8>> {
    let mut prefixed = Vec::with_capacity(HASH_PREFIX_TX_SIGN.len() + unsigned.len());
    prefixed.extend_from_slice(&HASH_PREFIX_TX_SIGN);
    prefixed.extend_from_slice(unsigned);

    let full_hash = Sha512::digest(&prefixed);
    let half_hash = &full_hash[..32];

    let signing_key = SigningKey::from_slice(private_key.as_bytes())
        .map_err(|e| DontYeetWalletError::Crypto(format!("invalid signing key: {e}")))?;

    let signature: Signature = signing_key
        .sign_prehash(half_hash)
        .map_err(|e| DontYeetWalletError::Crypto(format!("ECDSA sign failed: {e}")))?;

    // Post-sign verification mirroring the M.4.1 EVM safety pattern.
    signing_key
        .verifying_key()
        .verify_prehash(half_hash, &signature)
        .map_err(|e| DontYeetWalletError::Crypto(format!("post-sign verification failed: {e}")))?;

    let der = signature.to_der();
    Ok(der.as_bytes().to_vec())
}

/// Decode an XRPL `r...` address into its 20-byte account ID.
///
/// Decodes the XRPL custom Base58 form, verifies the version byte
/// (`0x00`) and the trailing 4-byte SHA-256d checksum, and returns
/// the 20-byte account ID. The same routine satisfies both server
/// and browser callers because the alphabet + checksum logic lives
/// in the always-on [`crate::keys`] module.
///
/// # Errors
/// Returns [`DontYeetWalletError::Chain`] if the decoded payload isn't
/// 25 bytes or the checksum / version byte don't match. The
/// underlying [`xrp_base58_decode`] also surfaces
/// [`DontYeetWalletError::Validation`] for unknown alphabet characters.
pub fn address_to_account_id(address: &str) -> Result<[u8; 20]> {
    let decoded = xrp_base58_decode(address)?;
    if decoded.len() != 25 {
        return Err(DontYeetWalletError::Chain(format!(
            "expected 25 decoded bytes, got {}",
            decoded.len()
        )));
    }

    if decoded[0] != 0x00 {
        return Err(DontYeetWalletError::Chain(format!(
            "XRP address version byte must be 0x00, got 0x{:02x}",
            decoded[0]
        )));
    }

    let payload = &decoded[..21];
    let checksum = &decoded[21..25];
    let h1 = Sha256::digest(payload);
    let h2 = Sha256::digest(h1);
    if checksum != &h2[..4] {
        return Err(DontYeetWalletError::Chain("XRP address checksum mismatch".into()));
    }

    let mut account_id = [0u8; 20];
    account_id.copy_from_slice(&decoded[1..21]);
    Ok(account_id)
}

// ---------------------------------------------------------------------------
// Internal: XRPL binary serialization
// ---------------------------------------------------------------------------

/// Serialize a Payment in XRPL binary format.
///
/// Field order is the canonical sort: `(type_code, field_code)`.
/// For a Payment that's `TransactionType, Flags, Sequence,
/// LastLedgerSequence, Amount, Fee, SigningPubKey,
/// [TxnSignature], Account, Destination`.
///
/// `signature` is `None` for the unsigned (signing) form, `Some` for
/// the fully signed broadcast form.
fn serialize_payment(params: &PaymentParams, signature: Option<&[u8]>) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256);

    // TransactionType (1, 2) = Payment
    write_field_header(&mut buf, TYPE_UINT16, FIELD_TRANSACTION_TYPE);
    buf.extend_from_slice(&TX_TYPE_PAYMENT.to_be_bytes());

    // Flags (2, 2) = 0
    write_field_header(&mut buf, TYPE_UINT32, FIELD_FLAGS);
    buf.extend_from_slice(&0u32.to_be_bytes());

    // Sequence (2, 4)
    write_field_header(&mut buf, TYPE_UINT32, FIELD_SEQUENCE);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "XRPL sequences fit in u32; > u32::MAX would imply a corrupt account"
    )]
    buf.extend_from_slice(&(params.sequence as u32).to_be_bytes());

    // LastLedgerSequence (2, 27) — field code >= 16, two-byte header
    write_field_header(&mut buf, TYPE_UINT32, FIELD_LAST_LEDGER_SEQ);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "XRPL ledger indices fit in u32 for the foreseeable future"
    )]
    buf.extend_from_slice(&(params.last_ledger_seq as u32).to_be_bytes());

    // Amount (6, 1) — native XRP drops
    write_field_header(&mut buf, TYPE_AMOUNT, FIELD_AMOUNT);
    buf.extend_from_slice(&encode_xrp_amount(params.drops));

    // Fee (6, 8)
    write_field_header(&mut buf, TYPE_AMOUNT, FIELD_FEE);
    buf.extend_from_slice(&encode_xrp_amount(params.fee_drops));

    // SigningPubKey (7, 3)
    write_field_header(&mut buf, TYPE_BLOB, FIELD_SIGNING_PUB_KEY);
    write_vl(&mut buf, &params.signing_pub_key);

    // TxnSignature (7, 4) — only in signed form.
    if let Some(sig) = signature {
        write_field_header(&mut buf, TYPE_BLOB, FIELD_TXN_SIGNATURE);
        write_vl(&mut buf, sig);
    }

    // Account (8, 1)
    write_field_header(&mut buf, TYPE_ACCOUNT, FIELD_ACCOUNT);
    write_vl(&mut buf, &params.from_account_id);

    // Destination (8, 3)
    write_field_header(&mut buf, TYPE_ACCOUNT, FIELD_DESTINATION);
    write_vl(&mut buf, &params.to_account_id);

    buf
}

/// Encode a field header.
///
/// If the field code fits in 4 bits: single byte `(type << 4) | field`.
/// Otherwise: two bytes `(type << 4) | 0` followed by the full field code.
fn write_field_header(buf: &mut Vec<u8>, type_code: u8, field_code: u8) {
    if field_code < 16 {
        buf.push((type_code << 4) | field_code);
    } else {
        buf.push(type_code << 4);
        buf.push(field_code);
    }
}

/// Encode a native-XRP drops amount as 8 big-endian bytes.
///
/// Bit 63 = 0 selects native XRP; bit 62 = 1 marks a positive amount.
fn encode_xrp_amount(drops: u64) -> [u8; 8] {
    (drops | 0x4000_0000_0000_0000).to_be_bytes()
}

/// Write a Variable-Length-encoded blob.
///
/// `len < 192`: single-byte length. `192..12479`: two-byte form.
/// XRPL also defines a three-byte form (>=12480) but we never emit
/// blobs that big from this code path.
fn write_vl(buf: &mut Vec<u8>, data: &[u8]) {
    let len = data.len();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "branches above bound the cast to single bytes"
    )]
    if len < 192 {
        buf.push(len as u8);
    } else {
        let adjusted = len - 192;
        buf.push(193 + (adjusted >> 8) as u8);
        buf.push((adjusted & 0xFF) as u8);
    }
    buf.extend_from_slice(data);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_key() -> PrivateKey {
        let key_bytes =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .expect("hex");
        PrivateKey::new(key_bytes)
    }

    fn fixture_pub_key() -> Vec<u8> {
        crate::keys::compressed_pubkey(&fixture_key()).expect("pubkey")
    }

    fn fixture_params() -> PaymentParams {
        PaymentParams {
            from_account_id: [0x11; 20],
            to_account_id: [0x22; 20],
            drops: 1_000_000,
            fee_drops: 12,
            sequence: 5,
            last_ledger_seq: 1_000_100,
            signing_pub_key: fixture_pub_key(),
        }
    }

    #[test]
    fn encode_xrp_amount_one_xrp() {
        let drops: u64 = 1_000_000;
        let encoded = encode_xrp_amount(drops);
        assert_eq!(encoded, (0x4000_0000_0000_0000u64 | drops).to_be_bytes());
    }

    #[test]
    fn encode_xrp_amount_zero() {
        assert_eq!(encode_xrp_amount(0), 0x4000_0000_0000_0000u64.to_be_bytes());
    }

    #[test]
    fn field_header_small_codes() {
        let mut buf = Vec::new();
        write_field_header(&mut buf, TYPE_UINT16, FIELD_TRANSACTION_TYPE);
        assert_eq!(buf, vec![0x12]); // (1 << 4) | 2
    }

    #[test]
    fn field_header_large_field_code() {
        let mut buf = Vec::new();
        write_field_header(&mut buf, TYPE_UINT32, FIELD_LAST_LEDGER_SEQ);
        assert_eq!(buf, vec![0x20, 27]); // (2 << 4) | 0, then 27
    }

    #[test]
    fn vl_short_length() {
        let mut buf = Vec::new();
        write_vl(&mut buf, &[0xAA; 33]); // 33-byte pubkey
        assert_eq!(buf[0], 33);
        assert_eq!(buf.len(), 34);
    }

    #[test]
    fn serialize_payment_unsigned_is_deterministic() {
        let p = fixture_params();
        assert_eq!(serialize_payment(&p, None), serialize_payment(&p, None));
    }

    #[test]
    fn serialize_payment_signed_is_longer_than_unsigned() {
        let p = fixture_params();
        let unsigned = serialize_payment(&p, None);
        let signed = serialize_payment(&p, Some(&[0xBB; 70]));
        assert!(signed.len() > unsigned.len());
    }

    #[test]
    fn sign_unsigned_payload_returns_der() {
        let pk = fixture_key();
        let sig = sign_unsigned_payload(b"fake unsigned blob", &pk).expect("sign");
        // DER signature is typically 70-72 bytes (no sighash suffix).
        assert!(sig.len() >= 67 && sig.len() <= 72, "got {}", sig.len());
    }

    #[test]
    fn sign_unsigned_payload_is_deterministic() {
        let pk = fixture_key();
        let s1 = sign_unsigned_payload(b"deterministic", &pk).expect("s1");
        let s2 = sign_unsigned_payload(b"deterministic", &pk).expect("s2");
        assert_eq!(s1, s2);
    }

    #[test]
    fn sign_unsigned_payload_rejects_zero_key() {
        let pk = PrivateKey::new(vec![0u8; 32]);
        assert!(matches!(
            sign_unsigned_payload(b"data", &pk),
            Err(DontYeetWalletError::Crypto(_))
        ));
    }

    #[test]
    fn build_signed_payment_round_trip() {
        let params = fixture_params();
        let pk = fixture_key();
        let blob = build_signed_payment(&params, &pk).expect("build");

        let unsigned_len = serialize_payment(&params, None).len();
        // Signed form must include a TxnSignature field (~70 bytes
        // DER + 1 VL byte) so it's strictly longer than the unsigned
        // form.
        assert!(blob.len() > unsigned_len + 60);
    }

    #[test]
    fn build_signed_payment_rejects_short_pubkey() {
        let mut params = fixture_params();
        params.signing_pub_key = vec![0u8; 32]; // 32 instead of 33
        let pk = fixture_key();
        assert!(matches!(
            build_signed_payment(&params, &pk),
            Err(DontYeetWalletError::Validation(_))
        ));
    }

    #[test]
    fn address_to_account_id_round_trips_with_encoder() {
        use dontyeet_primitives::chain::NetworkId;
        use dontyeet_primitives::traits::AddressEncoder;

        let pub_key = fixture_pub_key();
        let encoder = crate::keys::XrpAddressEncoder;
        let network = NetworkId::new("xrp-mainnet");
        let addr = encoder.encode(&pub_key, &network).expect("encode");

        let account_id = address_to_account_id(addr.as_str()).expect("decode");
        assert_eq!(account_id.len(), 20);
    }

    #[test]
    fn address_to_account_id_rejects_garbage() {
        // 'X'-prefixed garbage — won't decode cleanly.
        assert!(address_to_account_id("Xnotanaddress12345").is_err());
    }
}

// Rust guideline compliant 2026-02-21
