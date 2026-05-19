//! Kaspa key derivation and address encoding.
//!
//! Implements [`KeyDeriver`] and [`AddressEncoder`] for the Kaspa
//! `BlockDAG` chain.
//!
//! Address derivation:
//! 1. BIP-44 seed derivation at `m/44'/111111'/0'/0/0`
//! 2. secp256k1 compressed public key (33 bytes)
//! 3. SHA-256 hash of the compressed public key, take first 20 bytes
//! 4. Address = `kaspa:` + `hex(pubkey_hash)`

use k256::ecdsa::SigningKey;
use sha2::{Digest, Sha256};

use dontyeet_crypto::Bip44Deriver;
use dontyeet_primitives::address::Address;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::{KeyPair, PrivateKey, Seed};
use dontyeet_primitives::traits::{AddressEncoder, KeyDeriver};

use crate::config::KaspaConfig;

/// Derive a Kaspa mainnet address for the given seed and BIP-44 path.
///
/// Pure-crypto, no I/O. Address shape: `kaspa:` + hex-encoded
/// 20-byte SHA-256 prefix of the compressed secp256k1 pubkey.
///
/// # Errors
/// Returns `DontYeetWalletError::Crypto` if BIP-44 derivation or secp256k1
/// key construction fails.
pub fn derive_address(seed: &Seed, derivation_path: &str) -> Result<Address> {
    let private_key = Bip44Deriver::derive(seed, derivation_path)?;
    address_from_private_key(&private_key, &NetworkId::new("kaspa-mainnet"))
}

/// Kaspa mainnet address prefix.
const MAINNET_PREFIX: &str = "kaspa:";

/// Kaspa testnet address prefix.
const TESTNET_PREFIX: &str = "kaspatest:";

/// Length of a hex-encoded pubkey hash (20 bytes = 40 hex chars).
const PUBKEY_HASH_HEX_LEN: usize = 40;

/// Derives Kaspa keypairs from a seed using BIP-44.
pub struct KaspaKeyDeriver {
    derivation_path: String,
}

impl KaspaKeyDeriver {
    /// Create a new key deriver with the BIP-44 path from config.
    #[must_use]
    pub fn new(config: &KaspaConfig) -> Self {
        Self {
            derivation_path: config.derivation_path.clone(),
        }
    }
}

impl KeyDeriver for KaspaKeyDeriver {
    /// Derive a Kaspa [`KeyPair`] from the master seed.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Crypto` if BIP-44 derivation or secp256k1
    /// key construction fails.
    fn derive_keypair(&self, seed: &Seed, network: &NetworkId) -> Result<KeyPair> {
        let private_key = Bip44Deriver::derive(seed, &self.derivation_path)?;
        let address = address_from_private_key(&private_key, network)?;
        Ok(KeyPair::new(address, private_key))
    }
}

/// Encodes Kaspa addresses using `kaspa:` prefix + hex-encoded pubkey hash.
pub struct KaspaAddressEncoder;

impl AddressEncoder for KaspaAddressEncoder {
    /// Encode a compressed secp256k1 public key (33 bytes) into a Kaspa
    /// address.
    ///
    /// The address format is: `kaspa:` (mainnet) or `kaspatest:` (testnet)
    /// followed by the hex-encoded 20-byte SHA-256 hash of the public key.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Chain` if the public key length is invalid.
    fn encode(&self, public_key: &[u8], network: &NetworkId) -> Result<Address> {
        if public_key.len() != 33 {
            return Err(DontYeetWalletError::Chain(format!(
                "invalid Kaspa public key length: {} (expected 33 compressed)",
                public_key.len()
            )));
        }

        let hash = pubkey_hash(public_key);
        let prefix = network_prefix(network);
        Ok(Address::new(format!("{prefix}{}", hex::encode(hash))))
    }

    /// Validate a Kaspa address string.
    ///
    /// Checks:
    /// - Starts with `kaspa:` (mainnet) or `kaspatest:` (testnet)
    /// - Remainder is valid hex, exactly 40 characters (20 bytes)
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Validation` if the address is malformed.
    fn validate(&self, address: &str, network: &NetworkId) -> Result<()> {
        let prefix = network_prefix(network);

        let hex_part = address.strip_prefix(prefix).ok_or_else(|| {
            DontYeetWalletError::Validation(format!(
                "Kaspa address must start with \"{prefix}\", got \"{address}\""
            ))
        })?;

        if hex_part.len() != PUBKEY_HASH_HEX_LEN {
            return Err(DontYeetWalletError::Validation(format!(
                "Kaspa address hash must be {PUBKEY_HASH_HEX_LEN} hex chars, got {}",
                hex_part.len()
            )));
        }

        hex::decode(hex_part).map_err(|_| {
            DontYeetWalletError::Validation("Kaspa address contains invalid hex characters".into())
        })?;

        Ok(())
    }
}

/// Derive a Kaspa address from a 32-byte private key.
///
/// # Errors
/// Returns `DontYeetWalletError::Crypto` if the private key is invalid.
fn address_from_private_key(private_key: &PrivateKey, network: &NetworkId) -> Result<Address> {
    let signing_key = SigningKey::from_slice(private_key.as_bytes())
        .map_err(|e| DontYeetWalletError::Crypto(format!("invalid secp256k1 key: {e}")))?;

    let verifying_key = signing_key.verifying_key();
    let compressed = verifying_key.to_encoded_point(true);
    let pub_bytes = compressed.as_bytes();

    let hash = pubkey_hash(pub_bytes);
    let prefix = network_prefix(network);
    Ok(Address::new(format!("{prefix}{}", hex::encode(hash))))
}

/// SHA-256 hash of public key bytes, truncated to 20 bytes.
fn pubkey_hash(pubkey: &[u8]) -> [u8; 20] {
    let full_hash = Sha256::digest(pubkey);
    let mut result = [0u8; 20];
    result.copy_from_slice(&full_hash[..20]);
    result
}

/// Return the address prefix for the given network.
fn network_prefix(network: &NetworkId) -> &'static str {
    if network.0.contains("testnet") {
        TESTNET_PREFIX
    } else {
        MAINNET_PREFIX
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_from_compressed_pubkey() {
        // Derive a public key from a known private key.
        let key_bytes =
            hex::decode("4c0883a69102937d6231471b5dbb6204fe512961708279f16c4b0e0a9ab8024a")
                .expect("valid hex");
        let signing_key = SigningKey::from_slice(&key_bytes).expect("valid key");
        let verifying_key = signing_key.verifying_key();
        let compressed = verifying_key.to_encoded_point(true);
        let pub_bytes = compressed.as_bytes();

        let encoder = KaspaAddressEncoder;
        let network = NetworkId::new("kaspa-mainnet");
        let addr = encoder.encode(pub_bytes, &network).expect("encode");

        // Must start with kaspa: and have 40 hex chars after.
        assert!(addr.as_str().starts_with("kaspa:"));
        assert_eq!(addr.as_str().len(), 6 + 40); // "kaspa:" = 6 chars
    }

    #[test]
    fn encode_testnet_prefix() {
        let key_bytes =
            hex::decode("4c0883a69102937d6231471b5dbb6204fe512961708279f16c4b0e0a9ab8024a")
                .expect("valid hex");
        let signing_key = SigningKey::from_slice(&key_bytes).expect("valid key");
        let verifying_key = signing_key.verifying_key();
        let compressed = verifying_key.to_encoded_point(true);

        let encoder = KaspaAddressEncoder;
        let network = NetworkId::new("kaspa-testnet");
        let addr = encoder
            .encode(compressed.as_bytes(), &network)
            .expect("encode");

        assert!(addr.as_str().starts_with("kaspatest:"));
    }

    #[test]
    fn validate_valid_mainnet_address() {
        let encoder = KaspaAddressEncoder;
        let network = NetworkId::new("kaspa-mainnet");
        // 40 hex chars = 20 bytes pubkey hash
        let addr = format!("kaspa:{}", "ab".repeat(20));
        assert!(encoder.validate(&addr, &network).is_ok());
    }

    #[test]
    fn validate_wrong_prefix_fails() {
        let encoder = KaspaAddressEncoder;
        let network = NetworkId::new("kaspa-mainnet");
        let addr = format!("bitcoin:{}", "ab".repeat(20));
        assert!(encoder.validate(&addr, &network).is_err());
    }

    #[test]
    fn validate_wrong_length_fails() {
        let encoder = KaspaAddressEncoder;
        let network = NetworkId::new("kaspa-mainnet");
        let addr = "kaspa:1234";
        assert!(encoder.validate(addr, &network).is_err());
    }

    #[test]
    fn validate_invalid_hex_fails() {
        let encoder = KaspaAddressEncoder;
        let network = NetworkId::new("kaspa-mainnet");
        let addr = format!("kaspa:{}", "zz".repeat(20));
        assert!(encoder.validate(&addr, &network).is_err());
    }

    #[test]
    fn address_deterministic() {
        let key_bytes =
            hex::decode("4c0883a69102937d6231471b5dbb6204fe512961708279f16c4b0e0a9ab8024a")
                .expect("valid hex");
        let pk = PrivateKey::new(key_bytes.clone());
        let network = NetworkId::new("kaspa-mainnet");
        let addr1 = address_from_private_key(&pk, &network).expect("derive");

        let pk2 = PrivateKey::new(key_bytes);
        let addr2 = address_from_private_key(&pk2, &network).expect("derive");

        assert_eq!(addr1, addr2);
    }

    #[test]
    fn pubkey_hash_is_20_bytes() {
        let fake_pubkey = [0u8; 33];
        let hash = pubkey_hash(&fake_pubkey);
        assert_eq!(hash.len(), 20);
    }
}

// Rust guideline compliant 2026-05-02
