//! Pure-crypto TRON transaction signing.
//!
//! TRON's `raw_data` (returned as `raw_data_hex` by the `TronGrid`
//! `POST /wallet/createtransaction` endpoint) is the protobuf-serialized
//! `Transaction.raw_data` body. Signing is single-SHA-256 of those
//! bytes followed by secp256k1 ECDSA. The result is the 65-byte
//! recoverable signature (`r || s || v`) where `v` is the 0/1
//! recovery id (NOT EIP-155's 27/28 form). The TRON node verifies
//! by recovering the signer's public key from `(sighash, r, s, v)`
//! and matching it against the address embedded in `owner_address`,
//! so the trailing `v` byte is load-bearing — without it the network
//! rejects the broadcast.
//!
//! Lives outside the `feature = "rpc"` gate so the WASM bundle can
//! sign without pulling reqwest, tokio, or any other server-only
//! dependency. Phase M.4.3 mirrors the M.4.1 / M.4.2 split — both
//! [`crate::transfer`] (server) and [`crate::wasm::send`] (browser)
//! call [`sign_raw_data`] and [`attach_signature`] so the two
//! pipelines emit byte-for-byte identical signed JSON.
//!
//! ## Layout
//!
//! - [`sign_raw_data`] — SHA-256 → secp256k1 → 64-byte sig.
//! - [`attach_signature`] — splices a hex-encoded sig into the
//!   `signature` field of a transaction JSON object.
//! - [`sign_and_attach`] — convenience that pipes the two together
//!   given a `raw_data_hex` string and a `serde_json::Value`.

use k256::ecdsa::{
    RecoveryId, Signature, SigningKey, VerifyingKey, signature::hazmat::PrehashSigner,
};
use sha2::{Digest, Sha256};

use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::PrivateKey;

/// Sign a TRON `raw_data` byte blob, returning the 65-byte recoverable
/// signature.
///
/// Hashes `raw_data` with single SHA-256 (TRON's sighash) and signs
/// with secp256k1 ECDSA. Returns `r || s || v` where `v` is the
/// secp256k1 recovery id (0 or 1) — NOT the EIP-155 `27 / 28` form
/// some libraries emit, since TRON nodes expect raw `0 / 1`. After
/// signing, recovers the signer's public key from
/// `(sighash, signature, recovery_id)` and asserts it matches the
/// signer; this catches faulty signing hardware or memory errors
/// before the bad sig reaches the network.
///
/// # Errors
/// Returns [`DontYeetWalletError::Crypto`] if `private_key` is rejected by
/// secp256k1, signing fails, signature recovery fails, or post-sign
/// verification fails.
pub fn sign_raw_data(raw_data: &[u8], private_key: &PrivateKey) -> Result<Vec<u8>> {
    let sighash = Sha256::digest(raw_data);

    let signing_key = SigningKey::from_slice(private_key.as_bytes())
        .map_err(|e| DontYeetWalletError::Crypto(format!("invalid signing key: {e}")))?;

    let (signature, recovery_id): (Signature, RecoveryId) = signing_key
        .sign_prehash(sighash.as_slice())
        .map_err(|e| DontYeetWalletError::Crypto(format!("ECDSA sign failed: {e}")))?;

    // Post-sign verification: recover the public key from the
    // signature + recovery id and compare to the signer. Mirrors
    // the M.4.1 EVM safety pattern.
    let recovered = VerifyingKey::recover_from_prehash(sighash.as_slice(), &signature, recovery_id)
        .map_err(|e| DontYeetWalletError::Crypto(format!("signature recovery failed: {e}")))?;
    if recovered != *signing_key.verifying_key() {
        return Err(DontYeetWalletError::Crypto(
            "post-sign verification failed: recovered pubkey does not match signer".into(),
        ));
    }

    let mut out = signature.to_bytes().to_vec();
    out.push(recovery_id.to_byte());
    Ok(out)
}

/// Splice a `signature` array into a TRON transaction JSON.
///
/// `tx_json` must be the JSON object returned by
/// `POST /wallet/createtransaction` (one whose top level has
/// `raw_data_hex`, `txID`, etc.). The signature is hex-encoded and
/// placed in a single-element array under the `signature` key,
/// matching what `POST /wallet/broadcasttransaction` expects.
pub fn attach_signature(tx_json: &mut serde_json::Value, signature: &[u8]) {
    tx_json["signature"] = serde_json::json!([hex::encode(signature)]);
}

/// Decode `raw_data_hex`, sign it, and splice the result into `tx_json`.
///
/// Convenience wrapper around [`sign_raw_data`] + [`attach_signature`]
/// — what both [`crate::transfer`] and [`crate::wasm::send`] do once
/// they've fetched the unsigned tx envelope from the node.
///
/// # Errors
/// - [`DontYeetWalletError::Chain`] if `raw_data_hex` isn't valid hex.
/// - [`DontYeetWalletError::Crypto`] if signing fails (see
///   [`sign_raw_data`]).
pub fn sign_and_attach(
    tx_json: &mut serde_json::Value,
    raw_data_hex: &str,
    private_key: &PrivateKey,
) -> Result<()> {
    let raw_bytes = hex::decode(raw_data_hex)
        .map_err(|e| DontYeetWalletError::Chain(format!("raw_data_hex decode: {e}")))?;
    let signature = sign_raw_data(&raw_bytes, private_key)?;
    attach_signature(tx_json, &signature);
    Ok(())
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

    #[test]
    fn sign_raw_data_returns_65_byte_recoverable_signature() {
        let pk = fixture_key();
        let sig = sign_raw_data(b"fake tron raw_data", &pk).expect("sign");
        // r (32) || s (32) || v (1) = 65 bytes; v must be 0 or 1.
        assert_eq!(sig.len(), 65);
        let v = *sig.last().expect("non-empty");
        assert!(v == 0 || v == 1, "v must be 0 or 1, got {v}");
    }

    #[test]
    fn sign_raw_data_is_deterministic() {
        let pk = fixture_key();
        let sig1 = sign_raw_data(b"deterministic", &pk).expect("sign1");
        let sig2 = sign_raw_data(b"deterministic", &pk).expect("sign2");
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn sign_raw_data_rejects_zero_key() {
        let pk = PrivateKey::new(vec![0u8; 32]);
        assert!(matches!(
            sign_raw_data(b"data", &pk),
            Err(DontYeetWalletError::Crypto(_))
        ));
    }

    #[test]
    fn attach_signature_writes_single_element_array() {
        let mut tx = serde_json::json!({
            "raw_data_hex": "abcd",
            "txID": "deadbeef",
        });
        attach_signature(&mut tx, &[0xAAu8; 65]);

        let sigs = tx
            .get("signature")
            .and_then(|v| v.as_array())
            .expect("signature array");
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].as_str().expect("hex string"), "aa".repeat(65));
    }

    #[test]
    fn sign_and_attach_round_trip() {
        let pk = fixture_key();
        let raw_hex = "0a02abcd1234"; // arbitrary even-length hex
        let mut tx = serde_json::json!({"raw_data_hex": raw_hex});
        sign_and_attach(&mut tx, raw_hex, &pk).expect("sign + attach");

        let sigs = tx
            .get("signature")
            .and_then(|v| v.as_array())
            .expect("signature array");
        let sig_hex = sigs[0].as_str().expect("hex");
        // 65 bytes → 130 hex chars.
        assert_eq!(sig_hex.len(), 130);
    }

    #[test]
    fn sign_and_attach_rejects_invalid_hex() {
        let pk = fixture_key();
        let mut tx = serde_json::json!({});
        let err = sign_and_attach(&mut tx, "not-hex", &pk).expect_err("must fail");
        assert!(matches!(err, DontYeetWalletError::Chain(_)));
    }
}

// Rust guideline compliant 2026-02-21
