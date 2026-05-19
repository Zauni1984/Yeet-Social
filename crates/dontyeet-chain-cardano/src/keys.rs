//! Cardano key derivation and address encoding.
//!
//! Implements [`KeyDeriver`] and [`AddressEncoder`] for Cardano.
//!
//! Address derivation (enterprise address, type 6):
//! 1. CIP-1852 seed derivation at `m/1852'/1815'/0'/0/0`
//! 2. Ed25519 public key (32 bytes)
//! 3. Key hash = blake2b-224(pubkey) → 28 bytes
//! 4. Header byte: 0x61 (mainnet) or 0x60 (testnet)
//! 5. Bech32 encode with HRP `addr` (mainnet) / `addr_test` (testnet)

use blake2::digest::consts::U28;
use blake2::{Blake2b, Digest};
use ed25519_dalek::SigningKey;

use dontyeet_crypto::Bip44Deriver;
use dontyeet_primitives::address::Address;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::{KeyPair, PrivateKey, Seed};
use dontyeet_primitives::traits::{AddressEncoder, KeyDeriver};

use crate::config::CardanoConfig;

/// Blake2b-224 output type alias.
type Blake2b224 = Blake2b<U28>;

/// Derive a Cardano enterprise address (type 6) for the mainnet.
///
/// Pure-crypto, no I/O. Address shape: bech32 with HRP `addr`,
/// payload = `0x61 || blake2b-224(pubkey)` (header byte 0x61 marks
/// type 6 enterprise + mainnet network tag).
///
/// Enterprise (single-key) addresses are sufficient for receive +
/// balance use cases. The full Shelley base address (with separate
/// staking key) can come later when staking actions are wired up.
///
/// # Errors
/// Returns `DontYeetWalletError::Crypto` if BIP-44 derivation or ed25519
/// key construction fails, or `DontYeetWalletError::Chain` if bech32
/// encoding fails.
pub fn derive_address(seed: &Seed, derivation_path: &str) -> Result<Address> {
    let private_key = Bip44Deriver::derive(seed, derivation_path)?;
    let pub_bytes = ed25519_pubkey(&private_key)?;
    CardanoAddressEncoder.encode(&pub_bytes, &NetworkId::new("cardano-mainnet"))
}

/// Derives Cardano keypairs from a seed using CIP-1852.
pub struct CardanoKeyDeriver {
    derivation_path: String,
}

impl CardanoKeyDeriver {
    /// Create a new key deriver from the config's derivation path.
    #[must_use]
    pub fn new(config: &CardanoConfig) -> Self {
        Self {
            derivation_path: config.derivation_path.clone(),
        }
    }
}

impl KeyDeriver for CardanoKeyDeriver {
    /// Derive a Cardano [`KeyPair`] from the master seed.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Crypto` if BIP-44 derivation or Ed25519
    /// key construction fails.
    fn derive_keypair(&self, seed: &Seed, network: &NetworkId) -> Result<KeyPair> {
        let private_key = Bip44Deriver::derive(seed, &self.derivation_path)?;
        let pub_bytes = ed25519_pubkey(&private_key)?;
        let encoder = CardanoAddressEncoder;
        let address = encoder.encode(&pub_bytes, network)?;
        Ok(KeyPair::new(address, private_key))
    }
}

/// Encodes Cardano enterprise addresses (type 6, single key).
pub struct CardanoAddressEncoder;

impl AddressEncoder for CardanoAddressEncoder {
    /// Encode a 32-byte Ed25519 public key into a Cardano enterprise
    /// address.
    ///
    /// Uses blake2b-224 to hash the public key, prepends the network
    /// header byte, and Bech32-encodes the result.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Chain` if the public key length is
    /// invalid or Bech32 encoding fails.
    fn encode(&self, public_key: &[u8], network: &NetworkId) -> Result<Address> {
        if public_key.len() != 32 {
            return Err(DontYeetWalletError::Chain(format!(
                "expected 32-byte Ed25519 pubkey, got {}",
                public_key.len()
            )));
        }

        let key_hash = blake2b_224(public_key);
        let header = header_byte(network);

        // Enterprise address: header_byte + 28-byte key hash = 29 bytes.
        let mut raw = Vec::with_capacity(29);
        raw.push(header);
        raw.extend_from_slice(&key_hash);

        let hrp = hrp_for_network(network);
        let hrp_parsed = bech32::Hrp::parse(hrp)
            .map_err(|e| DontYeetWalletError::Chain(format!("invalid bech32 HRP: {e}")))?;
        let addr = bech32::encode::<bech32::Bech32>(hrp_parsed, &raw)
            .map_err(|e| DontYeetWalletError::Chain(format!("bech32 encode error: {e}")))?;

        Ok(Address::new(addr))
    }

    /// Validate a Cardano address string.
    ///
    /// Checks:
    /// - Valid Bech32 encoding
    /// - HRP is `addr` (mainnet) or `addr_test` (testnet)
    /// - Decoded payload is 29 bytes (enterprise address)
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Validation` if the address is malformed.
    fn validate(&self, address: &str, network: &NetworkId) -> Result<()> {
        let expected_hrp = hrp_for_network(network);

        let (hrp, data) = bech32::decode(address)
            .map_err(|e| DontYeetWalletError::Validation(format!("bech32 decode error: {e}")))?;

        if hrp.as_str() != expected_hrp {
            return Err(DontYeetWalletError::Validation(format!(
                "HRP mismatch: expected '{expected_hrp}', got '{hrp}'"
            )));
        }

        // Enterprise address is 29 bytes (1 header + 28 key hash).
        // Shelley base address is 57 bytes (1 + 28 + 28). Accept both.
        if data.len() != 29 && data.len() != 57 {
            return Err(DontYeetWalletError::Validation(format!(
                "Cardano address payload must be 29 or 57 bytes, got {}",
                data.len()
            )));
        }

        Ok(())
    }
}

/// Derive the 32-byte Ed25519 public key from a private key.
pub(crate) fn ed25519_pubkey(private_key: &PrivateKey) -> Result<Vec<u8>> {
    let key_bytes: [u8; 32] = private_key
        .as_bytes()
        .try_into()
        .map_err(|_| DontYeetWalletError::Crypto("Ed25519 key must be 32 bytes".into()))?;
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let verifying_key = signing_key.verifying_key();
    Ok(verifying_key.to_bytes().to_vec())
}

/// Compute blake2b-224 hash (28 bytes) of the input.
fn blake2b_224(data: &[u8]) -> [u8; 28] {
    let hash = Blake2b224::digest(data);
    let mut result = [0u8; 28];
    result.copy_from_slice(&hash);
    result
}

/// Enterprise address header byte.
///
/// Type 6 (enterprise) + network tag:
/// - 0x61 = type 6, mainnet (network tag 1)
/// - 0x60 = type 6, testnet (network tag 0)
fn header_byte(network: &NetworkId) -> u8 {
    if network.as_ref().contains("mainnet") {
        0x61
    } else {
        0x60
    }
}

/// Return the Bech32 HRP for the given Cardano network.
fn hrp_for_network(network: &NetworkId) -> &'static str {
    if network.as_ref().contains("mainnet") {
        "addr"
    } else {
        "addr_test"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blake2b_224_produces_28_bytes() {
        let hash = blake2b_224(&[0u8; 32]);
        assert_eq!(hash.len(), 28);
    }

    #[test]
    fn ed25519_pubkey_is_32_bytes() {
        let pk = PrivateKey::new(vec![1u8; 32]);
        let pub_key = ed25519_pubkey(&pk).expect("derive pubkey");
        assert_eq!(pub_key.len(), 32);
    }

    #[test]
    fn encode_mainnet_starts_with_addr() {
        let pk = PrivateKey::new(vec![1u8; 32]);
        let pub_key = ed25519_pubkey(&pk).expect("pubkey");

        let encoder = CardanoAddressEncoder;
        let network = NetworkId::new("cardano-mainnet");
        let addr = encoder.encode(&pub_key, &network).expect("encode");

        assert!(addr.as_str().starts_with("addr1"));
    }

    #[test]
    fn encode_testnet_starts_with_addr_test() {
        let pk = PrivateKey::new(vec![1u8; 32]);
        let pub_key = ed25519_pubkey(&pk).expect("pubkey");

        let encoder = CardanoAddressEncoder;
        let network = NetworkId::new("cardano-preprod");
        let addr = encoder.encode(&pub_key, &network).expect("encode");

        assert!(addr.as_str().starts_with("addr_test1"));
    }

    #[test]
    fn encode_then_validate_roundtrip() {
        let pk = PrivateKey::new(vec![42u8; 32]);
        let pub_key = ed25519_pubkey(&pk).expect("pubkey");

        let encoder = CardanoAddressEncoder;
        let network = NetworkId::new("cardano-mainnet");
        let addr = encoder.encode(&pub_key, &network).expect("encode");
        encoder.validate(addr.as_str(), &network).expect("valid");
    }

    #[test]
    fn validate_wrong_hrp_fails() {
        let pk = PrivateKey::new(vec![1u8; 32]);
        let pub_key = ed25519_pubkey(&pk).expect("pubkey");

        let encoder = CardanoAddressEncoder;
        let mainnet = NetworkId::new("cardano-mainnet");
        let preprod = NetworkId::new("cardano-preprod");

        let addr = encoder.encode(&pub_key, &mainnet).expect("encode");
        // Validating a mainnet address against testnet network should fail.
        assert!(encoder.validate(addr.as_str(), &preprod).is_err());
    }

    #[test]
    fn validate_invalid_address_fails() {
        let encoder = CardanoAddressEncoder;
        let network = NetworkId::new("cardano-mainnet");
        assert!(encoder.validate("not_an_address", &network).is_err());
    }

    #[test]
    fn header_byte_mainnet() {
        let network = NetworkId::new("cardano-mainnet");
        assert_eq!(header_byte(&network), 0x61);
    }

    #[test]
    fn header_byte_testnet() {
        let network = NetworkId::new("cardano-preprod");
        assert_eq!(header_byte(&network), 0x60);
    }
}

// Rust guideline compliant 2026-05-02
