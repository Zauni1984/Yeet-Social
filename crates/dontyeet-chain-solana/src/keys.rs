//! Solana key derivation and address encoding.
//!
//! Implements [`KeyDeriver`] and [`AddressEncoder`] for Solana.
//! Uses Ed25519 keys with Base58-encoded 32-byte public key addresses.

use ed25519_dalek::SigningKey;

use dontyeet_crypto::Bip44Deriver;
use dontyeet_primitives::address::Address;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::{KeyPair, PrivateKey, Seed};
use dontyeet_primitives::traits::{AddressEncoder, KeyDeriver};

use crate::config::SolConfig;

/// Derive a Solana address for the given seed and BIP-44 path.
///
/// Pure-crypto, no I/O — drop-in replacement for the
/// [`SolKeyDeriver`] machinery when the caller doesn't need the
/// full plugin trait (e.g. the WASM frontend showing a receive
/// address).
///
/// # Errors
/// Returns `DontYeetWalletError::Crypto` if BIP-44 derivation or Ed25519
/// key construction fails, or `DontYeetWalletError::Chain` if address
/// encoding fails.
pub fn derive_address(seed: &Seed, derivation_path: &str) -> Result<Address> {
    let private_key = Bip44Deriver::derive(seed, derivation_path)?;
    let pubkey_bytes = ed25519_pubkey(&private_key)?;
    SolAddressEncoder.encode(&pubkey_bytes, &NetworkId::new("solana-mainnet"))
}

// ---------------------------------------------------------------------------
// Key derivation
// ---------------------------------------------------------------------------

/// Derives Solana keypairs from a seed using BIP-44 (`m/44'/501'/0'/0'`).
pub struct SolKeyDeriver {
    derivation_path: String,
}

impl SolKeyDeriver {
    /// Create a new key deriver from the config's derivation path.
    #[must_use]
    pub fn new(config: &SolConfig) -> Self {
        Self {
            derivation_path: config.derivation_path.clone(),
        }
    }
}

impl KeyDeriver for SolKeyDeriver {
    /// Derive a Solana [`KeyPair`] from the master seed.
    ///
    /// Uses BIP-44 path `m/44'/501'/0'/0'` to derive a 32-byte private
    /// key, then constructs an Ed25519 signing key and derives the
    /// public key as the Solana address (Base58-encoded).
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Crypto` if BIP-44 derivation or Ed25519
    /// key construction fails.
    fn derive_keypair(&self, seed: &Seed, network: &NetworkId) -> Result<KeyPair> {
        let private_key = Bip44Deriver::derive(seed, &self.derivation_path)?;
        let pubkey_bytes = ed25519_pubkey(&private_key)?;
        let encoder = SolAddressEncoder;
        let address = encoder.encode(&pubkey_bytes, network)?;
        Ok(KeyPair::new(address, private_key))
    }
}

// ---------------------------------------------------------------------------
// Address encoding
// ---------------------------------------------------------------------------

/// Encodes Solana addresses as Base58-encoded 32-byte Ed25519 public keys.
///
/// Solana addresses are simply the Base58 representation of the raw
/// 32-byte public key with no hashing or checksum.
pub struct SolAddressEncoder;

impl AddressEncoder for SolAddressEncoder {
    /// Encode a 32-byte Ed25519 public key into a Base58 address.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Chain` if the public key length is not 32 bytes.
    fn encode(&self, public_key: &[u8], _network: &NetworkId) -> Result<Address> {
        if public_key.len() != 32 {
            return Err(DontYeetWalletError::Chain(format!(
                "expected 32-byte Ed25519 pubkey, got {}",
                public_key.len()
            )));
        }
        let addr = bs58::encode(public_key).into_string();
        Ok(Address::new(addr))
    }

    /// Validate a Solana address string.
    ///
    /// A valid Solana address is a Base58 string that decodes to exactly
    /// 32 bytes.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Validation` if the address is malformed.
    fn validate(&self, address: &str, _network: &NetworkId) -> Result<()> {
        let decoded = bs58::decode(address)
            .into_vec()
            .map_err(|e| DontYeetWalletError::Validation(format!("base58 decode error: {e}")))?;

        if decoded.len() != 32 {
            return Err(DontYeetWalletError::Validation(format!(
                "Solana address must decode to 32 bytes, got {}",
                decoded.len()
            )));
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Derive the 32-byte Ed25519 public key from a 32-byte private key.
///
/// # Errors
/// Returns `DontYeetWalletError::Crypto` if the key bytes are not 32 bytes.
pub(crate) fn ed25519_pubkey(private_key: &PrivateKey) -> Result<Vec<u8>> {
    let key_bytes: [u8; 32] = private_key.as_bytes().try_into().map_err(|_| {
        DontYeetWalletError::Crypto(format!(
            "expected 32-byte Ed25519 key, got {}",
            private_key.as_bytes().len()
        ))
    })?;

    let signing_key = SigningKey::from_bytes(&key_bytes);
    let verifying_key = signing_key.verifying_key();
    Ok(verifying_key.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ed25519_pubkey_is_32_bytes() {
        let key_bytes = vec![1u8; 32];
        let pk = PrivateKey::new(key_bytes);
        let pubkey = ed25519_pubkey(&pk).expect("derive pubkey");
        assert_eq!(pubkey.len(), 32);
    }

    #[test]
    fn address_from_known_key() {
        // Any valid 32-byte key should produce a Base58 address.
        let key_bytes = vec![42u8; 32];
        let pk = PrivateKey::new(key_bytes);
        let pubkey = ed25519_pubkey(&pk).expect("derive pubkey");

        let encoder = SolAddressEncoder;
        let network = NetworkId::new("solana-mainnet");
        let addr = encoder.encode(&pubkey, &network).expect("encode");

        // Solana addresses are 32-44 Base58 characters.
        assert!(addr.as_str().len() >= 32);
        assert!(addr.as_str().len() <= 44);
    }

    #[test]
    fn address_validation_roundtrip() {
        let key_bytes = vec![7u8; 32];
        let pk = PrivateKey::new(key_bytes);
        let pubkey = ed25519_pubkey(&pk).expect("derive pubkey");

        let encoder = SolAddressEncoder;
        let network = NetworkId::new("solana-mainnet");
        let addr = encoder.encode(&pubkey, &network).expect("encode");

        // The encoded address should validate successfully.
        encoder
            .validate(addr.as_str(), &network)
            .expect("roundtrip validation");
    }

    #[test]
    fn validate_invalid_address_fails() {
        let encoder = SolAddressEncoder;
        let network = NetworkId::new("solana-mainnet");
        // "ZZZZ" is valid Base58 but decodes to fewer than 32 bytes.
        assert!(encoder.validate("ZZZZ", &network).is_err());
    }

    #[test]
    fn validate_non_base58_fails() {
        let encoder = SolAddressEncoder;
        let network = NetworkId::new("solana-mainnet");
        assert!(encoder.validate("not+valid+base58!", &network).is_err());
    }

    #[test]
    fn key_derivation_produces_32_bytes() {
        use dontyeet_crypto::mnemonic::{Bip39Generator, WordCount};

        let m = Bip39Generator::generate(WordCount::Twelve).expect("mnemonic");
        let seed = Bip39Generator::to_seed(&m, "").expect("seed");
        let pk = Bip44Deriver::derive(&seed, dontyeet_crypto::derivation::paths::SOLANA)
            .expect("derive");
        assert_eq!(pk.as_bytes().len(), 32);
    }
}

// Rust guideline compliant 2026-05-02
