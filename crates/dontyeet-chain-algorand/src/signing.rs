//! Pure-crypto Algorand Payment encoding and Ed25519 signing.
//!
//! Hand-rolled canonical `MessagePack` encoder for the Payment
//! transaction shape (sorted keys, most-compact integer encoding,
//! binary-not-array byte slices, zero-value fields omitted), plus
//! `"TX"`-prefixed Ed25519 signing and the signed-transaction
//! envelope. Both server-side [`crate::transfer`] and the in-browser
//! [`crate::wasm::send`] pre-fetch suggested params from Algod's
//! `/v2/transactions/params` endpoint, then call
//! [`build_signed_payment`] to produce the wire-format blob.
//!
//! Lives outside the `feature = "rpc"` gate so browser consumers
//! (`default-features = false`) can sign without pulling in
//! `reqwest`, `tokio`, or any other server-only dependency. Phase
//! M.4.6 mirrors the M.4.1 / M.4.2 / M.4.3 / M.4.4 / M.4.5 split:
//! extract the protocol logic into a shared module, then have both
//! the rpc-feature glue and the wasm glue delegate to it.
//!
//! ## Layout
//!
//! - [`PaymentParams`] — caller-supplied inputs: amount, fee,
//!   round window, genesis id + hash, sender + recipient pubkeys.
//! - [`build_signed_payment`] — full pipeline: encode unsigned
//!   payment as canonical msgpack → `"TX"`-prefix + Ed25519 sign +
//!   verify → splice into the `{ "sig": ..., "txn": ... }`
//!   signed-transaction envelope.
//! - [`address_to_pubkey`] — Algorand `Base32(pubkey || checksum)`
//!   address → 32-byte Ed25519 public key.
//! - [`sign_unsigned_payload`] — `"TX"`-prefix + Ed25519 + post-sign
//!   verify primitive, re-used by [`crate::tx::AlgoTransactionSigner`]
//!   so the `ChainPlugin::signer()` path goes through identical code.

use ed25519_dalek::{Signer, SigningKey, Verifier};

use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::PrivateKey;

/// Algorand domain-separator prefix prepended to the canonical
/// msgpack body before Ed25519 signing.
const TX_PREFIX: &[u8] = b"TX";

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Caller-supplied inputs for an Algorand Payment transaction.
#[derive(Debug, Clone)]
pub struct PaymentParams {
    /// 32-byte Ed25519 public key of the sender (account ID).
    pub sender: [u8; 32],
    /// 32-byte Ed25519 public key of the recipient.
    pub receiver: [u8; 32],
    /// Amount to transfer in microAlgos (1 ALGO = 1e6 microAlgos).
    pub micro_algos: u64,
    /// Fee to pay in microAlgos. Algod returns `min_fee` of
    /// 1000 `µALGO` from `/v2/transactions/params`.
    pub fee: u64,
    /// First valid round. Use the `last-round` field returned by
    /// `/v2/transactions/params`.
    pub first_valid: u64,
    /// Last valid round. Use `first_valid + 1000` for a default
    /// ~50-minute validity window.
    pub last_valid: u64,
    /// Genesis ID string (`"mainnet-v1.0"` for mainnet).
    pub genesis_id: String,
    /// 32-byte genesis hash (Base64-decoded from the value
    /// `/v2/transactions/params` returns).
    pub genesis_hash: [u8; 32],
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Build, sign, and assemble an Algorand Payment transaction.
///
/// Pipeline: canonical msgpack of the payment fields →
/// [`sign_unsigned_payload`] (`"TX"` prefix + Ed25519 + post-sign
/// verify) → splice into the signed-transaction envelope
/// `{ "sig": <sig>, "txn": <unsigned> }`. Returns the bytes ready
/// for broadcast via Algod's `POST /v2/transactions`.
///
/// # Errors
/// Returns [`DontYeetWalletError::Crypto`] if `private_key` isn't 32 bytes
/// or post-sign verification fails.
pub fn build_signed_payment(params: &PaymentParams, private_key: &PrivateKey) -> Result<Vec<u8>> {
    let unsigned = encode_payment(params);
    let signature = sign_unsigned_payload(&unsigned, private_key)?;
    Ok(encode_signed_tx(&signature, &unsigned))
}

/// Sign an Algorand canonical-msgpack body with the `"TX"` prefix.
///
/// Prepends the 2-byte domain separator `b"TX"`, signs the result
/// with Ed25519, and verifies the produced signature against the
/// signer's public key before returning. Mirrors the M.4.1 EVM
/// safety pattern.
///
/// # Errors
/// Returns [`DontYeetWalletError::Crypto`] if `private_key` isn't 32 bytes
/// or post-sign verification fails.
pub fn sign_unsigned_payload(unsigned: &[u8], private_key: &PrivateKey) -> Result<[u8; 64]> {
    let key_bytes: [u8; 32] = private_key
        .as_bytes()
        .try_into()
        .map_err(|_| DontYeetWalletError::Crypto("Ed25519 key must be 32 bytes".into()))?;

    let signing_key = SigningKey::from_bytes(&key_bytes);

    let mut prefixed = Vec::with_capacity(TX_PREFIX.len() + unsigned.len());
    prefixed.extend_from_slice(TX_PREFIX);
    prefixed.extend_from_slice(unsigned);

    let signature = signing_key.sign(&prefixed);

    // Post-sign verification: catches faulty signing hardware or
    // memory errors before the bad sig reaches the network.
    signing_key
        .verifying_key()
        .verify(&prefixed, &signature)
        .map_err(|e| DontYeetWalletError::Crypto(format!("post-sign verification failed: {e}")))?;

    Ok(signature.to_bytes())
}

/// Decode an Algorand Base32 address into the underlying 32-byte
/// Ed25519 public key.
///
/// Algorand addresses are `Base32(pubkey[32] || checksum[4])` where
/// the checksum is `SHA-512/256(pubkey)[28..32]`. This routine
/// only extracts the public key — checksum verification is
/// performed by [`crate::keys::AlgoAddressEncoder::validate`].
///
/// # Errors
/// Returns [`DontYeetWalletError::Chain`] if the Base32 string is malformed
/// or doesn't decode to exactly 36 bytes.
pub fn address_to_pubkey(address: &str) -> Result<[u8; 32]> {
    let decoded = data_encoding::BASE32_NOPAD
        .decode(address.as_bytes())
        .map_err(|e| DontYeetWalletError::Chain(format!("Algorand address decode: {e}")))?;
    if decoded.len() != 36 {
        return Err(DontYeetWalletError::Chain(format!(
            "expected 36 decoded bytes, got {}",
            decoded.len()
        )));
    }
    let mut pubkey = [0u8; 32];
    pubkey.copy_from_slice(&decoded[..32]);
    Ok(pubkey)
}

// ---------------------------------------------------------------------------
// Internal: canonical msgpack encoding
// ---------------------------------------------------------------------------
//
// Algorand requires canonical `MessagePack`: sorted keys, most
// compact integer encoding, binary (not array) for byte slices, and
// zero-value fields omitted. We hand-encode to avoid pulling in
// `serde_bytes` / `rmp-serde` as always-on dependencies (`rmp-serde`
// stays gated behind `feature = "rpc"`).

/// Encode a Payment as canonical msgpack with sorted field names.
///
/// Field order is alphabetical: `amt`, `fee`, `fv`, `gen`, `gh`,
/// `lv`, `rcv`, `snd`, `type`. All fields are present; canonical
/// rules also require zero-value fields omitted, but for a Payment
/// these fields are always non-zero in practice (a zero-amt or
/// zero-fee transaction wouldn't be useful).
fn encode_payment(p: &PaymentParams) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256);

    // fixmap with 9 entries
    buf.push(0x89);

    write_fixstr(&mut buf, "amt");
    write_uint(&mut buf, p.micro_algos);

    write_fixstr(&mut buf, "fee");
    write_uint(&mut buf, p.fee);

    write_fixstr(&mut buf, "fv");
    write_uint(&mut buf, p.first_valid);

    write_fixstr(&mut buf, "gen");
    write_str(&mut buf, &p.genesis_id);

    write_fixstr(&mut buf, "gh");
    write_bin(&mut buf, &p.genesis_hash);

    write_fixstr(&mut buf, "lv");
    write_uint(&mut buf, p.last_valid);

    write_fixstr(&mut buf, "rcv");
    write_bin(&mut buf, &p.receiver);

    write_fixstr(&mut buf, "snd");
    write_bin(&mut buf, &p.sender);

    write_fixstr(&mut buf, "type");
    write_fixstr(&mut buf, "pay");

    buf
}

/// Wrap a signed transaction: `{ "sig": <signature>, "txn": <unsigned> }`.
fn encode_signed_tx(signature: &[u8; 64], unsigned: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(2 + 5 + 64 + 5 + unsigned.len());
    buf.push(0x82); // fixmap with 2 entries

    write_fixstr(&mut buf, "sig");
    write_bin(&mut buf, signature);

    write_fixstr(&mut buf, "txn");
    buf.extend_from_slice(unsigned); // already valid msgpack

    buf
}

// ---- Low-level msgpack helpers ----

/// Write a fixstr (length < 32).
fn write_fixstr(buf: &mut Vec<u8>, s: &str) {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the < 32 length is guaranteed by every fixstr call site"
    )]
    let header = 0xA0 | (s.len() as u8);
    buf.push(header);
    buf.extend_from_slice(s.as_bytes());
}

/// Write a string (handles up to 255 bytes via `str8`).
fn write_str(buf: &mut Vec<u8>, s: &str) {
    if s.len() < 32 {
        write_fixstr(buf, s);
    } else {
        buf.push(0xD9); // str 8
        #[expect(
            clippy::cast_possible_truncation,
            reason = "this branch only fires for lengths < 256"
        )]
        let len_byte = s.len() as u8;
        buf.push(len_byte);
        buf.extend_from_slice(s.as_bytes());
    }
}

/// Write a uint in the most compact `MessagePack` form.
fn write_uint(buf: &mut Vec<u8>, v: u64) {
    if v < 128 {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the < 128 branch ensures the cast fits"
        )]
        let byte = v as u8;
        buf.push(byte);
    } else if let Ok(b) = u8::try_from(v) {
        buf.push(0xCC);
        buf.push(b);
    } else if let Ok(h) = u16::try_from(v) {
        buf.push(0xCD);
        buf.extend_from_slice(&h.to_be_bytes());
    } else if let Ok(w) = u32::try_from(v) {
        buf.push(0xCE);
        buf.extend_from_slice(&w.to_be_bytes());
    } else {
        buf.push(0xCF);
        buf.extend_from_slice(&v.to_be_bytes());
    }
}

/// Write binary data (`bin 8` for length < 256).
fn write_bin(buf: &mut Vec<u8>, data: &[u8]) {
    buf.push(0xC4); // bin 8
    #[expect(
        clippy::cast_possible_truncation,
        reason = "all call sites pass <= 64 bytes (signature) or 32 bytes (pubkey/hash)"
    )]
    let len_byte = data.len() as u8;
    buf.push(len_byte);
    buf.extend_from_slice(data);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_key() -> PrivateKey {
        PrivateKey::new(vec![1u8; 32])
    }

    fn fixture_params() -> PaymentParams {
        PaymentParams {
            sender: [0x11; 32],
            receiver: [0x22; 32],
            micro_algos: 1_000_000,
            fee: 1_000,
            first_valid: 100,
            last_valid: 1_100,
            genesis_id: "mainnet-v1.0".into(),
            genesis_hash: [0x33; 32],
        }
    }

    #[test]
    fn write_uint_compact_fixint() {
        let mut buf = Vec::new();
        write_uint(&mut buf, 42);
        assert_eq!(buf, vec![42]);
    }

    #[test]
    fn write_uint_compact_u8() {
        let mut buf = Vec::new();
        write_uint(&mut buf, 200);
        assert_eq!(buf, vec![0xCC, 200]);
    }

    #[test]
    fn write_uint_compact_u16() {
        let mut buf = Vec::new();
        write_uint(&mut buf, 1000);
        assert_eq!(buf, vec![0xCD, 0x03, 0xE8]);
    }

    #[test]
    fn write_uint_compact_u32() {
        let mut buf = Vec::new();
        write_uint(&mut buf, 100_000);
        assert_eq!(buf, vec![0xCE, 0x00, 0x01, 0x86, 0xA0]);
    }

    #[test]
    fn write_uint_compact_u64() {
        let mut buf = Vec::new();
        write_uint(&mut buf, 5_000_000_000);
        assert_eq!(
            buf,
            vec![0xCF, 0x00, 0x00, 0x00, 0x01, 0x2A, 0x05, 0xF2, 0x00]
        );
    }

    #[test]
    fn write_bin_32_bytes() {
        let mut buf = Vec::new();
        write_bin(&mut buf, &[0xAA; 32]);
        assert_eq!(buf[0], 0xC4); // bin 8
        assert_eq!(buf[1], 32);
        assert_eq!(buf.len(), 34);
    }

    #[test]
    fn encode_payment_starts_with_fixmap_9() {
        let buf = encode_payment(&fixture_params());
        assert_eq!(buf[0], 0x89); // fixmap(9)
    }

    #[test]
    fn encode_signed_tx_starts_with_fixmap_2() {
        let unsigned = encode_payment(&fixture_params());
        let signed = encode_signed_tx(&[0xBBu8; 64], &unsigned);
        assert_eq!(signed[0], 0x82); // fixmap(2)
    }

    #[test]
    fn sign_unsigned_payload_returns_64_bytes() {
        let pk = fixture_key();
        let sig = sign_unsigned_payload(b"fake payload", &pk).expect("sign");
        assert_eq!(sig.len(), 64);
    }

    #[test]
    fn sign_unsigned_payload_is_deterministic() {
        let pk = fixture_key();
        let s1 = sign_unsigned_payload(b"deterministic", &pk).expect("s1");
        let s2 = sign_unsigned_payload(b"deterministic", &pk).expect("s2");
        assert_eq!(s1, s2);
    }

    #[test]
    fn sign_unsigned_payload_rejects_short_key() {
        let pk = PrivateKey::new(vec![1u8; 31]);
        assert!(matches!(
            sign_unsigned_payload(b"data", &pk),
            Err(DontYeetWalletError::Crypto(_))
        ));
    }

    #[test]
    fn build_signed_payment_round_trip() {
        let pk = fixture_key();
        let blob = build_signed_payment(&fixture_params(), &pk).expect("build");

        // Signed envelope must include the unsigned msgpack body
        // plus the 64-byte sig and ~10 bytes of envelope overhead.
        assert!(blob.len() > 64 + 32 + 32 + 32);
        assert_eq!(blob[0], 0x82); // fixmap(2): "sig" + "txn"
    }

    #[test]
    fn address_to_pubkey_round_trip_with_encoder() {
        use dontyeet_primitives::chain::NetworkId;
        use dontyeet_primitives::traits::AddressEncoder;

        let pubkey = [42u8; 32];
        let encoder = crate::keys::AlgoAddressEncoder;
        let network = NetworkId::new("algorand-mainnet");
        let addr = encoder.encode(&pubkey, &network).expect("encode");

        let decoded = address_to_pubkey(addr.as_str()).expect("decode");
        assert_eq!(decoded, pubkey);
    }

    #[test]
    fn address_to_pubkey_rejects_bad_base32() {
        // Padding char '=' isn't in Algorand's no-pad alphabet.
        assert!(address_to_pubkey("==invalid==").is_err());
    }
}

// Rust guideline compliant 2026-02-21
