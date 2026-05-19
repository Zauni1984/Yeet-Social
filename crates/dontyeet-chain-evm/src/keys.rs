//! EVM key derivation and EIP-55 address encoding.
//!
//! Implements [`KeyDeriver`] and [`AddressEncoder`] for all EVM chains.
//! Address derivation: private key -> secp256k1 public key -> Keccak-256 -> last 20 bytes -> EIP-55 checksum.

use k256::ecdsa::SigningKey;
use sha3::{Digest, Keccak256};

use dontyeet_crypto::Bip44Deriver;
use dontyeet_primitives::address::Address;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::{KeyPair, PrivateKey, Seed};
use dontyeet_primitives::traits::{AddressEncoder, KeyDeriver};

use crate::config::EvmChainConfig;

/// Derive the EIP-55 EVM address for the given seed and BIP-44 path.
///
/// Pure-crypto, no I/O — call sites that don't want the full
/// [`EvmKeyDeriver`] / [`KeyPair`] machinery (e.g. the WASM frontend
/// that only needs to display the user's address) can use this
/// directly.
///
/// # Errors
/// Returns `DontYeetWalletError::Crypto` if BIP-44 derivation fails or if
/// the derived private key is rejected by secp256k1.
pub fn derive_address(seed: &Seed, derivation_path: &str) -> Result<Address> {
    let private_key = Bip44Deriver::derive(seed, derivation_path)?;
    address_from_private_key(&private_key)
}

/// Derive the full EVM [`KeyPair`] (address + private key) from a
/// seed and BIP-44 path.
///
/// Pure-crypto, no I/O. Used by the WASM-side
/// [`crate::wasm::send`](crate::wasm) signing pipeline which needs
/// both halves of the keypair without instantiating a full
/// [`EvmChainConfig`]-backed [`EvmKeyDeriver`].
///
/// # Errors
/// Returns `DontYeetWalletError::Crypto` if BIP-44 derivation fails or if
/// the derived private key is rejected by secp256k1.
pub fn derive_keypair(seed: &Seed, derivation_path: &str) -> Result<KeyPair> {
    let private_key = Bip44Deriver::derive(seed, derivation_path)?;
    let address = address_from_private_key(&private_key)?;
    Ok(KeyPair::new(address, private_key))
}

/// Derives EVM keypairs from a seed using BIP-44.
pub struct EvmKeyDeriver {
    derivation_path: String,
}

impl EvmKeyDeriver {
    /// Create a new key deriver with the given BIP-44 path.
    #[must_use]
    pub fn new(config: &EvmChainConfig) -> Self {
        Self {
            derivation_path: config.derivation_path.clone(),
        }
    }
}

impl KeyDeriver for EvmKeyDeriver {
    /// Derive an EVM [`KeyPair`] from the master seed.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Crypto` if BIP-44 derivation or secp256k1
    /// key construction fails.
    fn derive_keypair(&self, seed: &Seed, _network: &NetworkId) -> Result<KeyPair> {
        let private_key = Bip44Deriver::derive(seed, &self.derivation_path)?;
        let address = address_from_private_key(&private_key)?;
        Ok(KeyPair::new(address, private_key))
    }
}

/// Encodes EVM addresses with EIP-55 checksumming.
pub struct EvmAddressEncoder;

impl AddressEncoder for EvmAddressEncoder {
    /// Encode a raw uncompressed public key (65 bytes with 0x04 prefix, or 64
    /// bytes without) into an EIP-55 checksummed address.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Chain` if the public key length is invalid.
    fn encode(&self, public_key: &[u8], _network: &NetworkId) -> Result<Address> {
        let key_bytes = match public_key.len() {
            65 => &public_key[1..],
            64 => public_key,
            n => {
                return Err(DontYeetWalletError::Chain(format!(
                    "invalid EVM public key length: {n} (expected 64 or 65)"
                )));
            }
        };
        let hash = Keccak256::digest(key_bytes);
        let addr_hex = hex::encode(&hash[12..]);
        let checksummed = eip55_checksum(&addr_hex);
        Ok(Address::new(format!("0x{checksummed}")))
    }

    /// Validate an EVM address string (format + EIP-55 checksum).
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Validation` if the address is malformed or
    /// fails the EIP-55 checksum.
    fn validate(&self, address: &str, _network: &NetworkId) -> Result<()> {
        if !address.starts_with("0x") && !address.starts_with("0X") {
            return Err(DontYeetWalletError::Validation(
                "EVM address must start with 0x".into(),
            ));
        }
        if address.len() != 42 {
            return Err(DontYeetWalletError::Validation(format!(
                "EVM address must be 42 chars, got {}",
                address.len()
            )));
        }
        let hex_part = &address[2..];
        if hex::decode(hex_part).is_err() {
            return Err(DontYeetWalletError::Validation(
                "EVM address contains invalid hex characters".into(),
            ));
        }
        // If the address is all-lowercase or all-uppercase, skip checksum
        // validation (valid per EIP-55).
        let is_all_lower = hex_part.chars().all(|c| !c.is_ascii_uppercase());
        let is_all_upper = hex_part.chars().all(|c| !c.is_ascii_lowercase());
        if is_all_lower || is_all_upper {
            return Ok(());
        }
        // Mixed case: verify EIP-55 checksum.
        let expected = eip55_checksum(&hex_part.to_ascii_lowercase());
        if hex_part != expected {
            return Err(DontYeetWalletError::Validation(
                "EVM address EIP-55 checksum mismatch".into(),
            ));
        }
        Ok(())
    }
}

/// Derive an EVM address from a 32-byte private key.
///
/// # Errors
/// Returns `DontYeetWalletError::Crypto` if the private key is invalid.
fn address_from_private_key(private_key: &PrivateKey) -> Result<Address> {
    let signing_key = SigningKey::from_slice(private_key.as_bytes())
        .map_err(|e| DontYeetWalletError::Crypto(format!("invalid secp256k1 key: {e}")))?;

    let verifying_key = signing_key.verifying_key();
    let point = verifying_key.to_encoded_point(false);
    let pub_bytes = point.as_bytes();
    // pub_bytes is 65 bytes: 0x04 || x (32) || y (32)
    let hash = Keccak256::digest(&pub_bytes[1..]);
    let addr_hex = hex::encode(&hash[12..]);
    let checksummed = eip55_checksum(&addr_hex);
    Ok(Address::new(format!("0x{checksummed}")))
}

/// Apply the EIP-55 mixed-case checksum to a lowercase hex address (without
/// `0x` prefix).
///
/// For each character in the hex address, if the corresponding nibble in the
/// Keccak-256 hash of the lowercase address is >= 8, uppercase that character.
fn eip55_checksum(addr_lower: &str) -> String {
    let hash = Keccak256::digest(addr_lower.as_bytes());
    let hash_hex = hex::encode(hash);

    addr_lower
        .chars()
        .enumerate()
        .map(|(i, c)| {
            if c.is_ascii_alphabetic() {
                // Each hex char in hash_hex corresponds to a nibble.
                let nibble = u8::from_str_radix(&hash_hex[i..=i], 16).unwrap_or(0);
                if nibble >= 8 {
                    c.to_ascii_uppercase()
                } else {
                    c
                }
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eip55_known_vector() {
        // EIP-55 specification test vector (all-caps variant).
        let lower = "5aaeb6053f3e94c9b9a09f33669435e7ef1beaed";
        let result = eip55_checksum(lower);
        assert_eq!(result, "5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed");
    }

    #[test]
    fn eip55_vitalik_address() {
        // Vitalik's well-known donation address.
        let lower = "ab5801a7d398351b8be11c439e05c5b3259aec9b";
        let result = eip55_checksum(lower);
        assert_eq!(result, "Ab5801a7D398351b8bE11C439e05C5B3259aeC9B");
    }

    #[test]
    fn address_from_known_private_key() {
        // Derive address from a known private key and verify round-trip.
        let key_bytes =
            hex::decode("4c0883a69102937d6231471b5dbb6204fe512961708279f16c4b0e0a9ab8024a")
                .expect("valid hex");
        let pk = PrivateKey::new(key_bytes);
        let addr = address_from_private_key(&pk).expect("derive address");
        // Verify the result is a valid EIP-55 address.
        let encoder = EvmAddressEncoder;
        let network = NetworkId::new("ethereum-mainnet");
        assert!(encoder.validate(addr.as_str(), &network).is_ok());
        // Known output for this private key.
        assert_eq!(addr.as_str(), "0xDE7dF24CAC2217C0AA2970325EFcbD8d4656DD2b");
    }

    #[test]
    fn validate_valid_checksummed_address() {
        let encoder = EvmAddressEncoder;
        let network = NetworkId::new("ethereum-mainnet");
        assert!(
            encoder
                .validate("0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed", &network)
                .is_ok()
        );
    }

    #[test]
    fn validate_all_lowercase_is_valid() {
        let encoder = EvmAddressEncoder;
        let network = NetworkId::new("ethereum-mainnet");
        assert!(
            encoder
                .validate("0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaed", &network)
                .is_ok()
        );
    }

    #[test]
    fn validate_wrong_checksum_fails() {
        let encoder = EvmAddressEncoder;
        let network = NetworkId::new("ethereum-mainnet");
        // Flip one character's case to break checksum.
        assert!(
            encoder
                .validate("0x5AAEB6053F3E94C9b9A09f33669435E7Ef1BeAed", &network)
                .is_err()
        );
    }

    #[test]
    fn validate_wrong_length_fails() {
        let encoder = EvmAddressEncoder;
        let network = NetworkId::new("ethereum-mainnet");
        assert!(encoder.validate("0x1234", &network).is_err());
    }

    #[test]
    fn validate_no_prefix_fails() {
        let encoder = EvmAddressEncoder;
        let network = NetworkId::new("ethereum-mainnet");
        assert!(
            encoder
                .validate("5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed", &network)
                .is_err()
        );
    }

    #[test]
    fn encode_from_uncompressed_pubkey() {
        // Derive the public key from the known private key and check encoding.
        let key_bytes =
            hex::decode("4c0883a69102937d6231471b5dbb6204fe512961708279f16c4b0e0a9ab8024a")
                .expect("valid hex");
        let signing_key = SigningKey::from_slice(&key_bytes).expect("valid key");
        let verifying_key = signing_key.verifying_key();
        let point = verifying_key.to_encoded_point(false);
        let pub_bytes = point.as_bytes();

        let encoder = EvmAddressEncoder;
        let network = NetworkId::new("ethereum-mainnet");
        let addr = encoder.encode(pub_bytes, &network).expect("encode");
        assert_eq!(addr.as_str(), "0xDE7dF24CAC2217C0AA2970325EFcbD8d4656DD2b");
    }
}

// Rust guideline compliant 2026-05-02
