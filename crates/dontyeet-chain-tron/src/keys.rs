//! TRON key derivation and address encoding.
//!
//! Implements [`KeyDeriver`] and [`AddressEncoder`] for TRON.
//! Addresses use the `Base58Check` format with a `0x41` version byte,
//! resulting in 34-character strings starting with `T`.

use k256::ecdsa::SigningKey;
use sha2::{Digest, Sha256};
use sha3::Keccak256;

use dontyeet_crypto::Bip44Deriver;
use dontyeet_primitives::address::Address;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::{KeyPair, PrivateKey, Seed};
use dontyeet_primitives::traits::{AddressEncoder, KeyDeriver};

use crate::config::TronConfig;

/// TRON address version byte.
const TRON_VERSION_BYTE: u8 = 0x41;

/// Derive a TRON address for the given seed and BIP-44 path.
///
/// Pure-crypto, no I/O — drop-in replacement for the
/// [`TronKeyDeriver`] machinery when the caller doesn't need the
/// full plugin trait (e.g. the WASM frontend showing a receive
/// address).
///
/// # Errors
/// Returns `DontYeetWalletError::Crypto` if BIP-44 derivation or secp256k1
/// key construction fails, or `DontYeetWalletError::Chain` if address
/// encoding fails.
pub fn derive_address(seed: &Seed, derivation_path: &str) -> Result<Address> {
    let private_key = Bip44Deriver::derive(seed, derivation_path)?;
    let uncompressed_pub = uncompressed_pubkey(&private_key)?;
    TronAddressEncoder.encode(&uncompressed_pub, &NetworkId::new("tron-mainnet"))
}

// ---------------------------------------------------------------------------
// Key derivation
// ---------------------------------------------------------------------------

/// Derives TRON keypairs from a seed using BIP-44 (`m/44'/195'/0'/0/0`).
pub struct TronKeyDeriver {
    derivation_path: String,
}

impl TronKeyDeriver {
    /// Create a new key deriver from the config's derivation path.
    #[must_use]
    pub fn new(config: &TronConfig) -> Self {
        Self {
            derivation_path: config.derivation_path.clone(),
        }
    }
}

impl KeyDeriver for TronKeyDeriver {
    /// Derive a TRON [`KeyPair`] from the master seed.
    ///
    /// Uses BIP-44 with coin type 195 for TRON.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Crypto` if BIP-44 derivation or secp256k1
    /// key construction fails.
    fn derive_keypair(&self, seed: &Seed, network: &NetworkId) -> Result<KeyPair> {
        let private_key = Bip44Deriver::derive(seed, &self.derivation_path)?;
        let uncompressed_pub = uncompressed_pubkey(&private_key)?;
        let encoder = TronAddressEncoder;
        let address = encoder.encode(&uncompressed_pub, network)?;
        Ok(KeyPair::new(address, private_key))
    }
}

// ---------------------------------------------------------------------------
// Address encoding
// ---------------------------------------------------------------------------

/// Encodes TRON addresses from uncompressed secp256k1 public keys.
///
/// TRON address derivation:
/// 1. Take the 65-byte uncompressed public key (with `0x04` prefix)
/// 2. Keccak-256 hash the last 64 bytes (skip the `0x04` prefix)
/// 3. Take the last 20 bytes of the Keccak hash
/// 4. Prepend version byte `0x41`
/// 5. Compute checksum: `SHA256(SHA256(0x41 + 20_bytes))`, first 4 bytes
/// 6. Base58 encode `(0x41 + 20_bytes + 4_checksum_bytes)`
/// 7. Result: 34-character address starting with `T`
pub struct TronAddressEncoder;

impl AddressEncoder for TronAddressEncoder {
    /// Encode a 65-byte uncompressed public key into a TRON `Base58Check`
    /// address.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Chain` if the public key length is invalid.
    fn encode(&self, public_key: &[u8], _network: &NetworkId) -> Result<Address> {
        if public_key.len() != 65 {
            return Err(DontYeetWalletError::Chain(format!(
                "expected 65-byte uncompressed pubkey, got {}",
                public_key.len()
            )));
        }

        // Step 1: Keccak-256 hash the last 64 bytes (skip 0x04 prefix).
        let keccak_hash = Keccak256::digest(&public_key[1..]);

        // Step 2: Take the last 20 bytes.
        let mut address_bytes = [0u8; 21];
        address_bytes[0] = TRON_VERSION_BYTE;
        address_bytes[1..].copy_from_slice(&keccak_hash[12..]);

        // Step 3: Compute double-SHA256 checksum.
        let checksum = double_sha256_checksum(&address_bytes);

        // Step 4: Concatenate address_bytes + checksum and Base58 encode.
        let mut full = Vec::with_capacity(25);
        full.extend_from_slice(&address_bytes);
        full.extend_from_slice(&checksum);

        Ok(Address::new(bs58::encode(full).into_string()))
    }

    /// Validate a TRON address string.
    ///
    /// TRON addresses are `Base58Check` encoded with version byte `0x41`
    /// and always start with `T`. They are 34 characters long.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Validation` if the address is malformed.
    fn validate(&self, address: &str, _network: &NetworkId) -> Result<()> {
        if !address.starts_with('T') {
            return Err(DontYeetWalletError::Validation(format!(
                "TRON address must start with 'T', got '{}'",
                address.chars().next().unwrap_or('?')
            )));
        }

        if address.len() != 34 {
            return Err(DontYeetWalletError::Validation(format!(
                "TRON address must be 34 characters, got {}",
                address.len()
            )));
        }

        let decoded = bs58::decode(address)
            .into_vec()
            .map_err(|e| DontYeetWalletError::Validation(format!("base58 decode error: {e}")))?;

        if decoded.len() != 25 {
            return Err(DontYeetWalletError::Validation(format!(
                "decoded TRON address must be 25 bytes, got {}",
                decoded.len()
            )));
        }

        // Verify version byte.
        if decoded[0] != TRON_VERSION_BYTE {
            return Err(DontYeetWalletError::Validation(format!(
                "TRON version byte must be 0x41, got 0x{:02x}",
                decoded[0]
            )));
        }

        // Verify checksum.
        let payload = &decoded[..21];
        let checksum = &decoded[21..25];
        let expected = double_sha256_checksum(payload);
        if checksum != expected {
            return Err(DontYeetWalletError::Validation(
                "TRON address checksum mismatch".into(),
            ));
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compute the first 4 bytes of `SHA256(SHA256(data))`.
fn double_sha256_checksum(data: &[u8]) -> [u8; 4] {
    let hash1 = Sha256::digest(data);
    let hash2 = Sha256::digest(hash1);
    let mut out = [0u8; 4];
    out.copy_from_slice(&hash2[..4]);
    out
}

/// Derive the 65-byte uncompressed public key from a private key.
///
/// # Errors
/// Returns `DontYeetWalletError::Crypto` if the key bytes are invalid.
fn uncompressed_pubkey(private_key: &PrivateKey) -> Result<Vec<u8>> {
    let signing_key = SigningKey::from_slice(private_key.as_bytes())
        .map_err(|e| DontYeetWalletError::Crypto(format!("invalid secp256k1 key: {e}")))?;
    let verifying_key = signing_key.verifying_key();
    let point = verifying_key.to_encoded_point(false);
    Ok(point.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncompressed_pubkey_is_65_bytes() {
        let key_bytes =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .expect("valid hex");
        let pk = PrivateKey::new(key_bytes);
        let uncompressed = uncompressed_pubkey(&pk).expect("derive pubkey");
        assert_eq!(uncompressed.len(), 65);
        assert_eq!(uncompressed[0], 0x04);
    }

    #[test]
    fn known_key_produces_t_address() {
        let key_bytes =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .expect("valid hex");
        let pk = PrivateKey::new(key_bytes);
        let pub_key = uncompressed_pubkey(&pk).expect("pubkey");

        let encoder = TronAddressEncoder;
        let network = NetworkId::new("tron-mainnet");
        let addr = encoder.encode(&pub_key, &network).expect("encode");

        assert!(addr.as_str().starts_with('T'));
        assert_eq!(addr.as_str().len(), 34);
    }

    #[test]
    fn address_validation_valid() {
        let key_bytes =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .expect("valid hex");
        let pk = PrivateKey::new(key_bytes);
        let pub_key = uncompressed_pubkey(&pk).expect("pubkey");

        let encoder = TronAddressEncoder;
        let network = NetworkId::new("tron-mainnet");
        let addr = encoder.encode(&pub_key, &network).expect("encode");

        encoder
            .validate(addr.as_str(), &network)
            .expect("should be valid");
    }

    #[test]
    fn address_validation_invalid_prefix() {
        let encoder = TronAddressEncoder;
        let network = NetworkId::new("tron-mainnet");
        assert!(
            encoder
                .validate("1NotATronAddress1234567890abcdef", &network)
                .is_err()
        );
    }

    #[test]
    fn address_validation_invalid_length() {
        let encoder = TronAddressEncoder;
        let network = NetworkId::new("tron-mainnet");
        assert!(encoder.validate("Tshort", &network).is_err());
    }

    #[test]
    fn address_validation_bad_checksum() {
        let encoder = TronAddressEncoder;
        let network = NetworkId::new("tron-mainnet");
        // 34-char string starting with T but with garbage content.
        assert!(
            encoder
                .validate("T000000000000000000000000000000000", &network)
                .is_err()
        );
    }

    #[test]
    fn key_derivation_produces_32_bytes() {
        use dontyeet_crypto::mnemonic::{Bip39Generator, WordCount};

        let m = Bip39Generator::generate(WordCount::Twelve).expect("mnemonic");
        let seed = Bip39Generator::to_seed(&m, "").expect("seed");
        let pk =
            Bip44Deriver::derive(&seed, dontyeet_crypto::derivation::paths::TRON).expect("derive");
        assert_eq!(pk.as_bytes().len(), 32);
    }
}

// Rust guideline compliant 2026-05-02
