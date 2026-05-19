//! Kadena key derivation and address encoding.
//!
//! Implements [`KeyDeriver`] and [`AddressEncoder`] for the Kadena
//! Chainweb chain (community edition).
//!
//! Address derivation:
//! 1. BIP-44 seed derivation at `m/44'/626'/0'/0/0`
//! 2. ED25519 public key (32 bytes)
//! 3. Address = `k:` + `hex(public_key)`

use ed25519_dalek::SigningKey;

use dontyeet_crypto::Bip44Deriver;
use dontyeet_primitives::address::Address;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::{KeyPair, PrivateKey, Seed};
use dontyeet_primitives::traits::{AddressEncoder, KeyDeriver};

use crate::config::KadenaConfig;

/// Kadena `k:` account prefix.
const K_PREFIX: &str = "k:";

/// Derive a Kadena address for the given seed and BIP-44 path.
///
/// Pure-crypto, no I/O. Address shape is `k:` + hex-encoded
/// 32-byte ed25519 public key.
///
/// # Errors
/// Returns `DontYeetWalletError::Crypto` if BIP-44 derivation or ed25519
/// key construction fails.
pub fn derive_address(seed: &Seed, derivation_path: &str) -> Result<Address> {
    let private_key = Bip44Deriver::derive(seed, derivation_path)?;
    address_from_private_key(&private_key, &NetworkId::new("kadena-mainnet"))
}

/// Length of a hex-encoded ED25519 public key (32 bytes = 64 hex chars).
const PUBKEY_HEX_LEN: usize = 64;

/// Derives Kadena keypairs from a seed using BIP-44.
pub struct KadenaKeyDeriver {
    derivation_path: String,
}

impl KadenaKeyDeriver {
    /// Create a new key deriver with the BIP-44 path from config.
    #[must_use]
    pub fn new(config: &KadenaConfig) -> Self {
        Self {
            derivation_path: config.derivation_path.clone(),
        }
    }
}

impl KeyDeriver for KadenaKeyDeriver {
    /// Derive a Kadena [`KeyPair`] from the master seed.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Crypto` if BIP-44 derivation or ED25519
    /// key construction fails.
    fn derive_keypair(&self, seed: &Seed, network: &NetworkId) -> Result<KeyPair> {
        let private_key = Bip44Deriver::derive(seed, &self.derivation_path)?;
        let address = address_from_private_key(&private_key, network)?;
        Ok(KeyPair::new(address, private_key))
    }
}

/// Encodes Kadena addresses using `k:` prefix + hex-encoded ED25519 public key.
pub struct KadenaAddressEncoder;

impl AddressEncoder for KadenaAddressEncoder {
    /// Encode an ED25519 public key (32 bytes) into a Kadena address.
    ///
    /// The address format is: `k:` followed by the hex-encoded 32-byte
    /// ED25519 public key.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Chain` if the public key length is invalid.
    fn encode(&self, public_key: &[u8], _network: &NetworkId) -> Result<Address> {
        if public_key.len() != 32 {
            return Err(DontYeetWalletError::Chain(format!(
                "invalid Kadena public key length: {} (expected 32 for ED25519)",
                public_key.len()
            )));
        }

        Ok(Address::new(format!(
            "{K_PREFIX}{}",
            hex::encode(public_key)
        )))
    }

    /// Validate a Kadena address string.
    ///
    /// Checks:
    /// - Starts with `k:`
    /// - Remainder is valid hex, exactly 64 characters (32 bytes)
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Validation` if the address is malformed.
    fn validate(&self, address: &str, _network: &NetworkId) -> Result<()> {
        let hex_part = address.strip_prefix(K_PREFIX).ok_or_else(|| {
            DontYeetWalletError::Validation(format!(
                "Kadena address must start with \"{K_PREFIX}\", got \"{address}\""
            ))
        })?;

        if hex_part.len() != PUBKEY_HEX_LEN {
            return Err(DontYeetWalletError::Validation(format!(
                "Kadena address key must be {PUBKEY_HEX_LEN} hex chars, got {}",
                hex_part.len()
            )));
        }

        hex::decode(hex_part).map_err(|_| {
            DontYeetWalletError::Validation("Kadena address contains invalid hex characters".into())
        })?;

        Ok(())
    }
}

/// Derive a Kadena address from a 32-byte private key (used as ED25519 seed).
///
/// # Errors
/// Returns `DontYeetWalletError::Crypto` if the private key is invalid.
fn address_from_private_key(private_key: &PrivateKey, _network: &NetworkId) -> Result<Address> {
    let key_bytes: [u8; 32] = private_key.as_bytes().try_into().map_err(|_| {
        DontYeetWalletError::Crypto(format!(
            "invalid ED25519 key length: {} (expected 32)",
            private_key.as_bytes().len()
        ))
    })?;

    let signing_key = SigningKey::from_bytes(&key_bytes);
    let verifying_key = signing_key.verifying_key();
    let pub_bytes = verifying_key.as_bytes();

    Ok(Address::new(format!(
        "{K_PREFIX}{}",
        hex::encode(pub_bytes)
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_from_ed25519_pubkey() {
        let key_bytes = [42u8; 32];
        let signing_key = SigningKey::from_bytes(&key_bytes);
        let verifying_key = signing_key.verifying_key();
        let pub_bytes = verifying_key.as_bytes();

        let encoder = KadenaAddressEncoder;
        let network = NetworkId::new("kadena-mainnet");
        let addr = encoder.encode(pub_bytes, &network).expect("encode");

        assert!(addr.as_str().starts_with("k:"));
        assert_eq!(addr.as_str().len(), 2 + 64); // "k:" = 2 chars + 64 hex
    }

    #[test]
    fn validate_valid_address() {
        let encoder = KadenaAddressEncoder;
        let network = NetworkId::new("kadena-mainnet");
        let addr = format!("k:{}", "ab".repeat(32));
        assert!(encoder.validate(&addr, &network).is_ok());
    }

    #[test]
    fn validate_wrong_prefix_fails() {
        let encoder = KadenaAddressEncoder;
        let network = NetworkId::new("kadena-mainnet");
        let addr = format!("w:{}", "ab".repeat(32));
        assert!(encoder.validate(&addr, &network).is_err());
    }

    #[test]
    fn validate_wrong_length_fails() {
        let encoder = KadenaAddressEncoder;
        let network = NetworkId::new("kadena-mainnet");
        let addr = "k:1234";
        assert!(encoder.validate(addr, &network).is_err());
    }

    #[test]
    fn validate_invalid_hex_fails() {
        let encoder = KadenaAddressEncoder;
        let network = NetworkId::new("kadena-mainnet");
        let addr = format!("k:{}", "zz".repeat(32));
        assert!(encoder.validate(&addr, &network).is_err());
    }

    #[test]
    fn address_deterministic() {
        let key_bytes = [42u8; 32];
        let pk = PrivateKey::new(key_bytes.to_vec());
        let network = NetworkId::new("kadena-mainnet");
        let addr1 = address_from_private_key(&pk, &network).expect("derive");

        let pk2 = PrivateKey::new(key_bytes.to_vec());
        let addr2 = address_from_private_key(&pk2, &network).expect("derive");

        assert_eq!(addr1, addr2);
    }
}

// Rust guideline compliant 2026-05-02
