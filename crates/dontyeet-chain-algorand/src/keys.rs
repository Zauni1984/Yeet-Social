//! Algorand key derivation and address encoding.
//!
//! Implements [`KeyDeriver`] and [`AddressEncoder`] for Algorand.
//!
//! Address derivation:
//! 1. BIP-44 seed derivation at `m/44'/283'/0'/0/0`
//! 2. Ed25519 public key (32 bytes)
//! 3. Address = Base32(pubkey + `last_4_bytes_of_SHA512_256(pubkey)`)
//!    → 58 uppercase characters

use ed25519_dalek::SigningKey;
use sha2::{Digest, Sha512_256};

use dontyeet_crypto::Bip44Deriver;
use dontyeet_primitives::address::Address;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::{KeyPair, PrivateKey, Seed};
use dontyeet_primitives::traits::{AddressEncoder, KeyDeriver};

use crate::config::AlgoConfig;

/// Expected length of an Algorand address string (Base32, 58 chars).
const ALGO_ADDRESS_LEN: usize = 58;

/// Derive an Algorand address for the given seed and BIP-44 path.
///
/// Pure-crypto, no I/O. Address shape: Base32 (no padding,
/// uppercase) of `pubkey || SHA-512/256(pubkey)[28..32]`. Always
/// 58 characters.
///
/// # Errors
/// Returns `DontYeetWalletError::Crypto` if BIP-44 derivation or ed25519
/// key construction fails.
pub fn derive_address(seed: &Seed, derivation_path: &str) -> Result<Address> {
    let private_key = Bip44Deriver::derive(seed, derivation_path)?;
    let pub_bytes = ed25519_pubkey(&private_key)?;
    AlgoAddressEncoder.encode(&pub_bytes, &NetworkId::new("algorand-mainnet"))
}

/// Derives Algorand keypairs from a seed using BIP-44.
pub struct AlgoKeyDeriver {
    derivation_path: String,
}

impl AlgoKeyDeriver {
    /// Create a new key deriver from the config's derivation path.
    #[must_use]
    pub fn new(config: &AlgoConfig) -> Self {
        Self {
            derivation_path: config.derivation_path.clone(),
        }
    }
}

impl KeyDeriver for AlgoKeyDeriver {
    /// Derive an Algorand [`KeyPair`] from the master seed.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Crypto` if BIP-44 derivation or Ed25519
    /// key construction fails.
    fn derive_keypair(&self, seed: &Seed, network: &NetworkId) -> Result<KeyPair> {
        let private_key = Bip44Deriver::derive(seed, &self.derivation_path)?;
        let pub_bytes = ed25519_pubkey(&private_key)?;
        let encoder = AlgoAddressEncoder;
        let address = encoder.encode(&pub_bytes, network)?;
        Ok(KeyPair::new(address, private_key))
    }
}

/// Encodes Algorand addresses using Base32(pubkey + checksum).
pub struct AlgoAddressEncoder;

impl AddressEncoder for AlgoAddressEncoder {
    /// Encode a 32-byte Ed25519 public key into an Algorand address.
    ///
    /// The address is Base32-encoded (uppercase, no padding) from
    /// 36 bytes: 32-byte public key + 4-byte checksum.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Chain` if the public key length is invalid.
    fn encode(&self, public_key: &[u8], _network: &NetworkId) -> Result<Address> {
        if public_key.len() != 32 {
            return Err(DontYeetWalletError::Chain(format!(
                "expected 32-byte Ed25519 pubkey, got {}",
                public_key.len()
            )));
        }

        let checksum = algo_checksum(public_key);
        let mut raw = Vec::with_capacity(36);
        raw.extend_from_slice(public_key);
        raw.extend_from_slice(&checksum);

        let addr = data_encoding::BASE32_NOPAD.encode(&raw);
        Ok(Address::new(addr))
    }

    /// Validate an Algorand address string.
    ///
    /// Checks:
    /// - Exactly 58 characters (Base32, no padding)
    /// - Valid Base32 decoding to 36 bytes
    /// - Last 4 bytes match SHA-512/256 checksum of first 32 bytes
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Validation` if the address is malformed.
    fn validate(&self, address: &str, _network: &NetworkId) -> Result<()> {
        if address.len() != ALGO_ADDRESS_LEN {
            return Err(DontYeetWalletError::Validation(format!(
                "Algorand address must be {ALGO_ADDRESS_LEN} chars, got {}",
                address.len()
            )));
        }

        let decoded = data_encoding::BASE32_NOPAD
            .decode(address.as_bytes())
            .map_err(|e| DontYeetWalletError::Validation(format!("Base32 decode error: {e}")))?;

        if decoded.len() != 36 {
            return Err(DontYeetWalletError::Validation(format!(
                "decoded address must be 36 bytes, got {}",
                decoded.len()
            )));
        }

        let pubkey = &decoded[..32];
        let checksum = &decoded[32..36];
        let expected = algo_checksum(pubkey);

        if checksum != expected {
            return Err(DontYeetWalletError::Validation(
                "Algorand address checksum mismatch".into(),
            ));
        }

        Ok(())
    }
}

/// Derive the 32-byte Ed25519 public key from a private key.
///
/// # Errors
/// Returns `DontYeetWalletError::Crypto` if the key bytes are invalid.
pub(crate) fn ed25519_pubkey(private_key: &PrivateKey) -> Result<Vec<u8>> {
    let key_bytes: [u8; 32] = private_key
        .as_bytes()
        .try_into()
        .map_err(|_| DontYeetWalletError::Crypto("Ed25519 key must be 32 bytes".into()))?;
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let verifying_key = signing_key.verifying_key();
    Ok(verifying_key.to_bytes().to_vec())
}

/// Compute the Algorand 4-byte checksum: last 4 bytes of SHA-512/256.
fn algo_checksum(pubkey: &[u8]) -> [u8; 4] {
    let hash = Sha512_256::digest(pubkey);
    let mut result = [0u8; 4];
    result.copy_from_slice(&hash[28..32]);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ed25519_pubkey_is_32_bytes() {
        let key_bytes = [1u8; 32];
        let pk = PrivateKey::new(key_bytes.to_vec());
        let pub_key = ed25519_pubkey(&pk).expect("derive pubkey");
        assert_eq!(pub_key.len(), 32);
    }

    #[test]
    fn encode_produces_58_char_address() {
        let key_bytes = [1u8; 32];
        let pk = PrivateKey::new(key_bytes.to_vec());
        let pub_key = ed25519_pubkey(&pk).expect("pubkey");

        let encoder = AlgoAddressEncoder;
        let network = NetworkId::new("algorand-mainnet");
        let addr = encoder.encode(&pub_key, &network).expect("encode");

        assert_eq!(addr.as_str().len(), 58);
    }

    #[test]
    fn encode_then_validate_roundtrip() {
        let key_bytes = [42u8; 32];
        let pk = PrivateKey::new(key_bytes.to_vec());
        let pub_key = ed25519_pubkey(&pk).expect("pubkey");

        let encoder = AlgoAddressEncoder;
        let network = NetworkId::new("algorand-mainnet");
        let addr = encoder.encode(&pub_key, &network).expect("encode");
        encoder.validate(addr.as_str(), &network).expect("valid");
    }

    #[test]
    fn validate_wrong_length_fails() {
        let encoder = AlgoAddressEncoder;
        let network = NetworkId::new("algorand-mainnet");
        assert!(encoder.validate("TOOOSHORT", &network).is_err());
    }

    #[test]
    fn validate_bad_checksum_fails() {
        let key_bytes = [1u8; 32];
        let pk = PrivateKey::new(key_bytes.to_vec());
        let pub_key = ed25519_pubkey(&pk).expect("pubkey");

        let encoder = AlgoAddressEncoder;
        let network = NetworkId::new("algorand-mainnet");
        let addr = encoder.encode(&pub_key, &network).expect("encode");

        // Flip last character to corrupt checksum.
        let mut corrupted = addr.as_str().to_string();
        let last = corrupted.pop().expect("non-empty");
        corrupted.push(if last == 'A' { 'B' } else { 'A' });

        assert!(encoder.validate(&corrupted, &network).is_err());
    }

    #[test]
    fn checksum_is_4_bytes() {
        let pubkey = [0u8; 32];
        let cs = algo_checksum(&pubkey);
        assert_eq!(cs.len(), 4);
    }
}

// Rust guideline compliant 2026-05-02
