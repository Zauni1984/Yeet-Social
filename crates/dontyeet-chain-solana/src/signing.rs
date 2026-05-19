//! Pure-crypto Solana versioned (v0) Transfer encoding and Ed25519 signing.
//!
//! Builds, signs, and serializes a single-instruction System-Program
//! `Transfer` v0 transaction without any I/O. Both the server-side
//! [`crate::transfer`] pipeline and the in-browser [`crate::wasm::send`]
//! entry point fetch a recent blockhash via JSON-RPC, hand the result
//! to [`build_signed_transfer`], and broadcast the returned bytes
//! through the `sendTransaction` RPC method.
//!
//! Lives outside the `feature = "rpc"` gate so browser consumers
//! (`default-features = false`) can sign without pulling in
//! `reqwest`, `tokio`, or any other server-only dependency. Phase
//! M.4.5 mirrors the M.4.1 / M.4.2 / M.4.3 / M.4.4 split: extract
//! the protocol logic into a shared module, then have both the
//! rpc-feature glue and the wasm glue delegate to it.
//!
//! ## Layout
//!
//! - [`build_transfer_message`] — versioned (v0) message body for a
//!   single System-Program `Transfer`. Pure function, no I/O.
//! - [`sign_message`] — Ed25519 over the raw message bytes (no
//!   sighash wrapper — Ed25519 includes SHA-512 internally) with
//!   post-sign verification mirroring the M.4.1 EVM safety pattern.
//! - [`assemble_transaction`] — splices `compact_u16(1) || sig ||
//!   message` into the wire-format blob the `sendTransaction`
//!   endpoint expects.
//! - [`build_signed_transfer`] — full pipeline given pre-fetched
//!   blockhash + sender / recipient pubkeys + amount.
//! - [`decode_address`] — Base58 → 32-byte Ed25519 public key.

use ed25519_dalek::{Signer, SigningKey, Verifier};

use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::PrivateKey;

/// System Program ID — 32 zero bytes.
const SYSTEM_PROGRAM_ID: [u8; 32] = [0u8; 32];

/// System Program `Transfer` instruction index (little-endian `u32`).
const TRANSFER_INSTRUCTION_INDEX: u32 = 2;

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Build, sign, and assemble a native-SOL Transfer transaction.
///
/// Pipeline: [`build_transfer_message`] for the v0 message body →
/// [`sign_message`] with Ed25519 + post-sign verify →
/// [`assemble_transaction`] for the wire format. Returns the bytes
/// ready for Base64 encoding + the `sendTransaction` JSON-RPC method.
///
/// # Errors
/// Returns [`DontYeetWalletError::Crypto`] if `private_key` isn't 32 bytes,
/// signing fails, or post-sign verification fails.
pub fn build_signed_transfer(
    from: &[u8; 32],
    to: &[u8; 32],
    recent_blockhash: &[u8; 32],
    lamports: u64,
    private_key: &PrivateKey,
) -> Result<Vec<u8>> {
    let message = build_transfer_message(from, to, recent_blockhash, lamports);
    let signature = sign_message(&message, private_key)?;
    Ok(assemble_transaction(&message, &signature))
}

/// Sign a Solana message with Ed25519 and return the 64-byte signature.
///
/// Solana signs the raw message bytes directly — Ed25519 includes
/// SHA-512 internally, so no sighash wrapper is needed. Verifies the
/// produced signature against the signer's public key before
/// returning, mirroring the M.4.1 EVM safety pattern.
///
/// # Errors
/// Returns [`DontYeetWalletError::Crypto`] if `private_key` isn't 32 bytes
/// or post-sign verification fails.
pub fn sign_message(message: &[u8], private_key: &PrivateKey) -> Result<[u8; 64]> {
    let key_bytes: [u8; 32] = private_key.as_bytes().try_into().map_err(|_| {
        DontYeetWalletError::Crypto(format!(
            "expected 32-byte Ed25519 key, got {}",
            private_key.as_bytes().len()
        ))
    })?;

    let signing_key = SigningKey::from_bytes(&key_bytes);
    let signature = signing_key.sign(message);

    // Post-sign verification: catches faulty signing hardware or
    // memory errors before the bad sig reaches the network.
    signing_key
        .verifying_key()
        .verify(message, &signature)
        .map_err(|e| DontYeetWalletError::Crypto(format!("post-sign verification failed: {e}")))?;

    Ok(signature.to_bytes())
}

/// Splice signature + message into the wire-format transaction.
///
/// Wire format: `compact_u16(1) || signature[64] || message`. The
/// `compact_u16` encoding for a 1-element list is a single `0x01`
/// byte, so the total length is always `1 + 64 + message.len()`.
#[must_use]
pub fn assemble_transaction(message: &[u8], signature: &[u8; 64]) -> Vec<u8> {
    let mut tx = Vec::with_capacity(1 + 64 + message.len());
    tx.push(0x01); // compact_u16(1) — one signature
    tx.extend_from_slice(signature);
    tx.extend_from_slice(message);
    tx
}

/// Build a versioned (v0) message body for a single System-Program
/// `Transfer` instruction.
///
/// Account layout:
/// - `[0]` sender (signer, writable)
/// - `[1]` recipient (writable)
/// - `[2]` System Program (read-only)
///
/// Wire format:
/// `[0x80 version] [header 3B] [accounts] [blockhash]`
/// `[instructions] [address_table_lookups]`
#[must_use]
pub fn build_transfer_message(
    from: &[u8; 32],
    to: &[u8; 32],
    recent_blockhash: &[u8; 32],
    lamports: u64,
) -> Vec<u8> {
    // 1 version + 3 header + 1 + 96 keys + 32 hash + 1 instr-count
    // + 16 instr + 1 ALT-count = 152.
    let mut msg = Vec::with_capacity(160);

    // ---- v0 version prefix ----
    // Bit 7 set marks a versioned message; lower 7 bits = 0 → v0.
    msg.push(0x80);

    // ---- Header ----
    msg.push(1); // num_required_signatures
    msg.push(0); // num_readonly_signed_accounts
    msg.push(1); // num_readonly_unsigned_accounts (System Program)

    // ---- Account keys (compact_u16(3) then 3 × 32 bytes) ----
    msg.push(3);
    msg.extend_from_slice(from);
    msg.extend_from_slice(to);
    msg.extend_from_slice(&SYSTEM_PROGRAM_ID);

    // ---- Recent blockhash ----
    msg.extend_from_slice(recent_blockhash);

    // ---- Instructions (compact_u16(1) then one Transfer) ----
    msg.push(1);
    msg.push(2); // program_id_index — System Program at account index 2
    msg.push(2); // compact_u16(2) — two account indices
    msg.push(0); // sender
    msg.push(1); // recipient

    // Instruction data: u32 LE instruction index + u64 LE lamports.
    msg.push(12); // compact_u16(12) — data length
    msg.extend_from_slice(&TRANSFER_INSTRUCTION_INDEX.to_le_bytes());
    msg.extend_from_slice(&lamports.to_le_bytes());

    // ---- Address Table Lookups (v0 section, empty for transfers) ----
    msg.push(0);

    msg
}

/// Decode a Solana Base58 address into the underlying 32-byte
/// Ed25519 public key.
///
/// # Errors
/// Returns [`DontYeetWalletError::Chain`] if the Base58 string is malformed
/// or doesn't decode to exactly 32 bytes.
pub fn decode_address(address: &str) -> Result<[u8; 32]> {
    let bytes = bs58::decode(address)
        .into_vec()
        .map_err(|e| DontYeetWalletError::Chain(format!("Solana address decode: {e}")))?;
    <[u8; 32]>::try_from(bytes.as_slice())
        .map_err(|_| DontYeetWalletError::Chain(format!("expected 32 bytes, got {}", bytes.len())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_key() -> PrivateKey {
        PrivateKey::new(vec![1u8; 32])
    }

    #[test]
    fn transfer_message_correct_length() {
        let msg = build_transfer_message(&[1; 32], &[2; 32], &[3; 32], 1_000_000_000);
        // 1 version + 3 header + 1 + 96 keys + 32 hash + 17 instr + 1 ALT = 152
        assert_eq!(msg.len(), 152);
    }

    #[test]
    fn transfer_message_version_and_header() {
        let msg = build_transfer_message(&[1; 32], &[2; 32], &[3; 32], 5000);
        assert_eq!(msg[0], 0x80); // v0 version prefix
        assert_eq!(msg[1], 1); // num_required_signatures
        assert_eq!(msg[2], 0); // num_readonly_signed
        assert_eq!(msg[3], 1); // num_readonly_unsigned
        assert_eq!(msg[4], 3); // 3 account keys
    }

    #[test]
    fn transfer_message_instruction_index() {
        let msg = build_transfer_message(&[1; 32], &[2; 32], &[3; 32], 5000);
        // 1 + 3 + 1 + 96 + 32 + 1 + 1 + 1 + 2 + 1 = 139
        let off = 139;
        let instr = u32::from_le_bytes([msg[off], msg[off + 1], msg[off + 2], msg[off + 3]]);
        assert_eq!(instr, TRANSFER_INSTRUCTION_INDEX);
    }

    #[test]
    fn transfer_message_lamports_encoding() {
        let lamports: u64 = 1_000_000_000;
        let msg = build_transfer_message(&[1; 32], &[2; 32], &[3; 32], lamports);
        let off = 143;
        let decoded = u64::from_le_bytes([
            msg[off],
            msg[off + 1],
            msg[off + 2],
            msg[off + 3],
            msg[off + 4],
            msg[off + 5],
            msg[off + 6],
            msg[off + 7],
        ]);
        assert_eq!(decoded, lamports);
    }

    #[test]
    fn transfer_message_alt_section_empty() {
        let msg = build_transfer_message(&[1; 32], &[2; 32], &[3; 32], 5000);
        assert_eq!(*msg.last().expect("non-empty"), 0x00);
    }

    #[test]
    fn sign_message_returns_64_bytes() {
        let pk = fixture_key();
        let sig = sign_message(b"fake solana message", &pk).expect("sign");
        assert_eq!(sig.len(), 64);
    }

    #[test]
    fn sign_message_is_deterministic() {
        let pk = fixture_key();
        let m = b"deterministic";
        let s1 = sign_message(m, &pk).expect("s1");
        let s2 = sign_message(m, &pk).expect("s2");
        assert_eq!(s1, s2);
    }

    #[test]
    fn sign_message_rejects_short_key() {
        let pk = PrivateKey::new(vec![1u8; 31]);
        assert!(matches!(
            sign_message(b"data", &pk),
            Err(DontYeetWalletError::Crypto(_))
        ));
    }

    #[test]
    fn assemble_transaction_layout() {
        let message = vec![0xAAu8; 152];
        let signature = [0xBBu8; 64];
        let tx = assemble_transaction(&message, &signature);
        assert_eq!(tx.len(), 1 + 64 + 152);
        assert_eq!(tx[0], 0x01); // compact_u16(1)
        assert_eq!(&tx[1..65], &signature[..]);
        assert_eq!(&tx[65..], &message[..]);
    }

    #[test]
    fn build_signed_transfer_round_trip() {
        let pk = fixture_key();
        let tx =
            build_signed_transfer(&[7; 32], &[8; 32], &[9; 32], 1_234_567, &pk).expect("build");
        // 1 + 64 + 152 = 217
        assert_eq!(tx.len(), 217);
        assert_eq!(tx[0], 0x01);
    }

    #[test]
    fn decode_address_round_trip() {
        let pubkey = [42u8; 32];
        let addr = bs58::encode(pubkey).into_string();
        assert_eq!(decode_address(&addr).expect("decode"), pubkey);
    }

    #[test]
    fn decode_address_rejects_short() {
        assert!(decode_address("11111").is_err());
    }
}

// Rust guideline compliant 2026-02-21
