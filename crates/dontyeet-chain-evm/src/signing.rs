//! Pure-crypto EVM transaction encoding and EIP-155 signing.
//!
//! Contains everything needed to turn `(nonce, gasPrice, gasLimit,
//! to, value, data, chainId)` into a fully signed RLP-encoded
//! transaction blob ready for `eth_sendRawTransaction`. No network
//! I/O, no side effects, no allocations beyond the output buffer —
//! safe to call from inside the WASM bundle (Phase M.4.1) or from
//! the server-side [`crate::tx`] pipeline (which it does as of this
//! refactor).
//!
//! Lives outside the [`feature = "rpc"`] gate so that browser
//! consumers (`default-features = false`) can sign transactions
//! without pulling in `reqwest`, `tokio`, or any other server-only
//! dependency.
//!
//! ## Layout
//!
//! - [`rlp_encode_unsigned`] — produce the EIP-155 unsigned blob
//!   `[nonce, gasPrice, gasLimit, to, value, data, chainId, 0, 0]`.
//! - [`sign_legacy_tx`] — Keccak-256 hash → secp256k1 ECDSA →
//!   re-encode with `(v, r, s)` per EIP-155 (`v = recId + chainId * 2 + 35`).
//! - Private helpers handle byte trimming and RLP re-encoding of the
//!   signed form.

use k256::ecdsa::{
    RecoveryId, Signature, SigningKey, VerifyingKey, signature::hazmat::PrehashSigner,
};
use sha3::{Digest, Keccak256};

use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::PrivateKey;

/// RLP-encode the EIP-155 *unsigned* form of a legacy EVM transaction.
///
/// The output is the 9-tuple
/// `[nonce, gasPrice, gasLimit, to, value, data, chainId, 0, 0]`
/// — exactly the bytes that get Keccak-256-hashed and signed by
/// [`sign_legacy_tx`].
#[must_use]
pub fn rlp_encode_unsigned(
    nonce: u64,
    gas_price: u128,
    gas_limit: u64,
    to: &[u8],
    value: u128,
    data: &[u8],
    evm_chain_id: u64,
) -> Vec<u8> {
    let mut stream = rlp::RlpStream::new_list(9);
    stream.append(&u64_to_be_trimmed(nonce));
    stream.append(&u128_to_be_trimmed(gas_price));
    stream.append(&u64_to_be_trimmed(gas_limit));
    stream.append(&to.to_vec());
    stream.append(&u128_to_be_trimmed(value));
    stream.append(&data.to_vec());
    stream.append(&u64_to_be_trimmed(evm_chain_id));
    stream.append(&Vec::<u8>::new());
    stream.append(&Vec::<u8>::new());
    stream.out().to_vec()
}

/// Sign an unsigned EIP-155 transaction blob and return its
/// signed RLP encoding.
///
/// Steps: Keccak-256 of `unsigned` → secp256k1 ECDSA prehash sign →
/// recover the public key from the signature and assert it matches
/// the signer (catches faulty signing hardware before broadcast) →
/// re-encode the original 6 fields plus `(v, r, s)` where
/// `v = recId + chainId * 2 + 35`.
///
/// # Errors
/// - [`DontYeetWalletError::Crypto`] if the private key is rejected by
///   secp256k1, signing fails, signature recovery fails, or the
///   recovered address doesn't match the signer.
/// - [`DontYeetWalletError::Chain`] on RLP decoding errors or `v`
///   arithmetic overflow.
pub fn sign_legacy_tx(
    unsigned: &[u8],
    private_key: &PrivateKey,
    evm_chain_id: u64,
) -> Result<Vec<u8>> {
    let hash = Keccak256::digest(unsigned);

    let signing_key = SigningKey::from_slice(private_key.as_bytes())
        .map_err(|e| DontYeetWalletError::Crypto(format!("invalid signing key: {e}")))?;

    let (signature, recovery_id): (Signature, RecoveryId) = signing_key
        .sign_prehash(hash.as_slice())
        .map_err(|e| DontYeetWalletError::Crypto(format!("ECDSA sign failed: {e}")))?;

    // Verify signature: recover the public key and check that the
    // derived address matches the signer. Catches corrupted recovery
    // IDs, faulty signing hardware, or memory errors before broadcast.
    let recovered_key =
        VerifyingKey::recover_from_prehash(hash.as_slice(), &signature, recovery_id)
            .map_err(|e| DontYeetWalletError::Crypto(format!("signature recovery failed: {e}")))?;

    let expected_addr = pubkey_to_evm_address(signing_key.verifying_key());
    let recovered_addr = pubkey_to_evm_address(&recovered_key);
    if expected_addr != recovered_addr {
        return Err(DontYeetWalletError::Crypto(
            "post-sign verification failed: recovered address does not match signer".into(),
        ));
    }

    let r_bytes = &signature.to_bytes()[..32];
    let s_bytes = &signature.to_bytes()[32..];

    // EIP-155: v = recovery_id + chain_id * 2 + 35
    let v = u64::from(recovery_id.to_byte())
        .checked_add(
            evm_chain_id
                .checked_mul(2)
                .ok_or_else(|| DontYeetWalletError::Chain("v calc overflow".into()))?,
        )
        .and_then(|val| val.checked_add(35))
        .ok_or_else(|| DontYeetWalletError::Chain("v calc overflow".into()))?;

    let decoded = rlp_decode_unsigned_fields(unsigned)?;
    Ok(rlp_encode_signed(&decoded, v, r_bytes, s_bytes))
}

// ---------------------------------------------------------------------------
// Private helpers — RLP re-encoding and byte trimming
// ---------------------------------------------------------------------------

/// RLP-encode the *signed* form: `[nonce, gasPrice, gasLimit, to,
/// value, data, v, r, s]`.
fn rlp_encode_signed(fields: &UnsignedFields, v: u64, r: &[u8], s: &[u8]) -> Vec<u8> {
    let mut stream = rlp::RlpStream::new_list(9);
    stream.append(&fields.nonce.clone());
    stream.append(&fields.gas_price.clone());
    stream.append(&fields.gas_limit.clone());
    stream.append(&fields.to.clone());
    stream.append(&fields.value.clone());
    stream.append(&fields.data.clone());
    stream.append(&u64_to_be_trimmed(v));
    stream.append(&trim_leading_zeros(r));
    stream.append(&trim_leading_zeros(s));
    stream.out().to_vec()
}

/// Decoded unsigned-tx fields, kept as raw byte vectors so they
/// round-trip through [`rlp_encode_signed`] unchanged.
struct UnsignedFields {
    nonce: Vec<u8>,
    gas_price: Vec<u8>,
    gas_limit: Vec<u8>,
    to: Vec<u8>,
    value: Vec<u8>,
    data: Vec<u8>,
}

/// Decode the first 6 fields from an EIP-155 unsigned RLP-encoded
/// transaction.
fn rlp_decode_unsigned_fields(encoded: &[u8]) -> Result<UnsignedFields> {
    let rlp = rlp::Rlp::new(encoded);
    let items: Vec<Vec<u8>> = rlp
        .as_list()
        .map_err(|e| DontYeetWalletError::Chain(format!("RLP decode error: {e}")))?;

    if items.len() < 6 {
        return Err(DontYeetWalletError::Chain(format!(
            "expected at least 6 RLP items, got {}",
            items.len()
        )));
    }

    Ok(UnsignedFields {
        nonce: items[0].clone(),
        gas_price: items[1].clone(),
        gas_limit: items[2].clone(),
        to: items[3].clone(),
        value: items[4].clone(),
        data: items[5].clone(),
    })
}

/// Encode a `u64` as big-endian bytes with leading zeros stripped.
///
/// RLP integers are minimal-length unsigned; zero is the empty
/// string.
fn u64_to_be_trimmed(val: u64) -> Vec<u8> {
    if val == 0 {
        return vec![];
    }
    trim_leading_zeros(&val.to_be_bytes())
}

/// Encode a `u128` as big-endian bytes with leading zeros stripped.
fn u128_to_be_trimmed(val: u128) -> Vec<u8> {
    if val == 0 {
        return vec![];
    }
    trim_leading_zeros(&val.to_be_bytes())
}

/// Strip leading zero bytes from a slice.
fn trim_leading_zeros(bytes: &[u8]) -> Vec<u8> {
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
    bytes[start..].to_vec()
}

/// Derive the 20-byte EVM address from an uncompressed secp256k1
/// public key.
///
/// Takes Keccak-256 of the 64-byte uncompressed key (without the
/// `0x04` prefix) and returns the last 20 bytes.
fn pubkey_to_evm_address(key: &VerifyingKey) -> [u8; 20] {
    let uncompressed = key.to_encoded_point(false);
    let hash = Keccak256::digest(&uncompressed.as_bytes()[1..]);
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&hash[12..]);
    addr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u64_trimmed_zero_is_empty() {
        assert!(u64_to_be_trimmed(0).is_empty());
    }

    #[test]
    fn u64_trimmed_small_value() {
        assert_eq!(u64_to_be_trimmed(1), vec![1]);
        assert_eq!(u64_to_be_trimmed(255), vec![255]);
        assert_eq!(u64_to_be_trimmed(256), vec![1, 0]);
    }

    #[test]
    fn u128_trimmed_large_value() {
        // 1 gwei = 1_000_000_000 = 0x3B9ACA00
        let result = u128_to_be_trimmed(1_000_000_000);
        assert_eq!(result, vec![0x3B, 0x9A, 0xCA, 0x00]);
    }

    #[test]
    fn roundtrip_unsigned_rlp() {
        let to_bytes = hex::decode("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045").expect("valid hex");
        let encoded = rlp_encode_unsigned(0, 20_000_000_000, 21_000, &to_bytes, 1_000_000, &[], 1);
        let decoded = rlp_decode_unsigned_fields(&encoded).expect("decode");
        assert_eq!(decoded.to, to_bytes);
    }
}

// Rust guideline compliant 2026-02-21
