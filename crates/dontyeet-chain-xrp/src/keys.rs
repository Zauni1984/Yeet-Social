//! XRP Ledger key derivation and address encoding.
//!
//! Implements [`KeyDeriver`] and [`AddressEncoder`] for the XRP Ledger.
//! Uses secp256k1 keys with BIP-44 path `m/44'/144'/0'/0/0` and
//! XRP's custom Base58 alphabet for address encoding.

use k256::ecdsa::SigningKey;
use ripemd::Ripemd160;
use sha2::{Digest, Sha256};

use dontyeet_crypto::Bip44Deriver;
use dontyeet_primitives::address::Address;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::{KeyPair, PrivateKey, Seed};
use dontyeet_primitives::traits::{AddressEncoder, KeyDeriver};

use crate::config::XrpConfig;

/// Derive an XRP Ledger address for the given seed and BIP-44 path.
///
/// Pure-crypto, no I/O. Address shape: XRP `Base58Check` with
/// version byte `0x00` over `RIPEMD160(SHA256(compressed_pubkey))`.
/// Always starts with `r`.
///
/// # Errors
/// Returns `DontYeetWalletError::Crypto` if BIP-44 derivation or secp256k1
/// key construction fails.
pub fn derive_address(seed: &Seed, derivation_path: &str) -> Result<Address> {
    let private_key = Bip44Deriver::derive(seed, derivation_path)?;
    let compressed_pub = compressed_pubkey(&private_key)?;
    XrpAddressEncoder.encode(&compressed_pub, &NetworkId::new("xrp-mainnet"))
}

/// XRP Ledger custom Base58 alphabet.
///
/// Different from the standard Bitcoin Base58 alphabet.
const XRP_ALPHABET: &[u8; 58] = b"rpshnaf39wBUDNEGHJKLM4PQRST7VWXYZ2bcdeCg65jkm8oFqi1tuvAxyz";

// ---------------------------------------------------------------------------
// Key derivation
// ---------------------------------------------------------------------------

/// Derives XRP keypairs from a seed using BIP-44 (`m/44'/144'/0'/0/0`).
pub struct XrpKeyDeriver {
    derivation_path: String,
}

impl XrpKeyDeriver {
    /// Create a new key deriver from the config's derivation path.
    #[must_use]
    pub fn new(config: &XrpConfig) -> Self {
        Self {
            derivation_path: config.derivation_path.clone(),
        }
    }
}

impl KeyDeriver for XrpKeyDeriver {
    /// Derive an XRP [`KeyPair`] from the master seed.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Crypto` if BIP-44 derivation or secp256k1
    /// key construction fails.
    fn derive_keypair(&self, seed: &Seed, network: &NetworkId) -> Result<KeyPair> {
        let private_key = Bip44Deriver::derive(seed, &self.derivation_path)?;
        let compressed_pub = compressed_pubkey(&private_key)?;
        let encoder = XrpAddressEncoder;
        let address = encoder.encode(&compressed_pub, network)?;
        Ok(KeyPair::new(address, private_key))
    }
}

// ---------------------------------------------------------------------------
// Address encoding
// ---------------------------------------------------------------------------

/// Encodes XRP Ledger addresses using the custom XRP Base58 alphabet.
///
/// Address format: version byte (0x00) + 20-byte account ID + 4-byte checksum.
/// The account ID is `RIPEMD160(SHA256(compressed_pubkey))`.
/// The checksum is the first 4 bytes of `SHA256(SHA256(version + account_id))`.
/// All XRP addresses start with `r`.
pub struct XrpAddressEncoder;

impl AddressEncoder for XrpAddressEncoder {
    /// Encode a 33-byte compressed public key into an XRP address.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Chain` if the public key length is invalid.
    fn encode(&self, public_key: &[u8], _network: &NetworkId) -> Result<Address> {
        if public_key.len() != 33 {
            return Err(DontYeetWalletError::Chain(format!(
                "expected 33-byte compressed pubkey, got {}",
                public_key.len()
            )));
        }

        let account_id = hash160(public_key);
        let addr = xrp_base58check_encode(0x00, &account_id);
        Ok(Address::new(addr))
    }

    /// Validate an XRP address string.
    ///
    /// XRP addresses must start with `r`, be 25-35 characters long,
    /// use only XRP Base58 alphabet characters, and have a valid checksum.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Validation` if the address is malformed.
    fn validate(&self, address: &str, _network: &NetworkId) -> Result<()> {
        if !address.starts_with('r') {
            return Err(DontYeetWalletError::Validation(
                "XRP address must start with 'r'".into(),
            ));
        }

        if address.len() < 25 || address.len() > 35 {
            return Err(DontYeetWalletError::Validation(format!(
                "XRP address length must be 25-35, got {}",
                address.len()
            )));
        }

        // Check all characters are in the XRP alphabet.
        for ch in address.chars() {
            if !XRP_ALPHABET.contains(&(ch as u8)) {
                return Err(DontYeetWalletError::Validation(format!(
                    "invalid character '{ch}' in XRP address"
                )));
            }
        }

        // Decode and verify checksum.
        let decoded = xrp_base58_decode(address)?;
        if decoded.len() != 25 {
            return Err(DontYeetWalletError::Validation(format!(
                "decoded XRP address must be 25 bytes, got {}",
                decoded.len()
            )));
        }

        let payload = &decoded[..21];
        let checksum = &decoded[21..25];
        let hash1 = Sha256::digest(payload);
        let hash2 = Sha256::digest(hash1);
        if checksum != &hash2[..4] {
            return Err(DontYeetWalletError::Validation(
                "XRP address checksum mismatch".into(),
            ));
        }

        // Version byte must be 0x00.
        if decoded[0] != 0x00 {
            return Err(DontYeetWalletError::Validation(format!(
                "XRP address version byte must be 0x00, got 0x{:02x}",
                decoded[0]
            )));
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// XRP custom Base58 encoding/decoding
// ---------------------------------------------------------------------------

/// Encode data with a version byte using XRP's custom `Base58Check`.
///
/// Format: `base58_xrp(version_byte || payload || checksum)` where
/// checksum = first 4 bytes of `SHA256(SHA256(version_byte || payload))`.
fn xrp_base58check_encode(version: u8, payload: &[u8]) -> String {
    let mut data = Vec::with_capacity(1 + payload.len() + 4);
    data.push(version);
    data.extend_from_slice(payload);

    let hash1 = Sha256::digest(&data);
    let hash2 = Sha256::digest(hash1);
    data.extend_from_slice(&hash2[..4]);

    xrp_base58_encode(&data)
}

/// Encode bytes using the XRP Base58 alphabet.
fn xrp_base58_encode(data: &[u8]) -> String {
    // Count leading zeros.
    let leading_zeros = data.iter().take_while(|&&b| b == 0).count();

    // Convert to big integer (big-endian bytes) and repeatedly divide by 58.
    let mut digits: Vec<u8> = Vec::new();
    let mut input = data.to_vec();

    while !input.is_empty() {
        let mut remainder: u32 = 0;
        let mut next = Vec::new();
        for &byte in &input {
            let value = u32::from(byte) + remainder * 256;
            let quotient = value / 58;
            remainder = value % 58;
            if !next.is_empty() || quotient > 0 {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "Base58 digit fits in u8 (quotient = value/58, value < 58*256, so quotient < 256)"
                )]
                next.push(quotient as u8);
            }
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "Base58 digit fits in u8 by construction (remainder = value % 58 < 58)"
        )]
        digits.push(remainder as u8);
        input = next;
    }

    // Map to XRP alphabet and reverse.
    let mut result = String::with_capacity(leading_zeros + digits.len());
    for _ in 0..leading_zeros {
        result.push(char::from(XRP_ALPHABET[0]));
    }
    for &d in digits.iter().rev() {
        result.push(char::from(XRP_ALPHABET[d as usize]));
    }
    result
}

/// Decode an XRP Base58-encoded string back to bytes.
///
/// # Errors
/// Returns `DontYeetWalletError::Validation` if the string contains invalid characters.
pub(crate) fn xrp_base58_decode(encoded: &str) -> Result<Vec<u8>> {
    // Build reverse lookup.
    let mut alphabet_map = [255u8; 128];
    for (i, &ch) in XRP_ALPHABET.iter().enumerate() {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "alphabet index fits in u8 (XRP_ALPHABET has 58 entries)"
        )]
        {
            alphabet_map[ch as usize] = i as u8;
        }
    }

    // Count leading 'r' (which maps to zero in XRP alphabet).
    let leading_zeros = encoded.chars().take_while(|&c| c == 'r').count();

    // Convert from base-58 to base-256.
    let mut bytes: Vec<u8> = Vec::new();
    for ch in encoded.chars() {
        let idx = ch as usize;
        if idx >= 128 || alphabet_map[idx] == 255 {
            return Err(DontYeetWalletError::Validation(format!(
                "invalid XRP Base58 character: '{ch}'"
            )));
        }
        let mut carry = u32::from(alphabet_map[idx]);
        for byte in &mut bytes {
            carry += u32::from(*byte) * 58;
            #[expect(
                clippy::cast_possible_truncation,
                reason = "intentional low-byte extraction; high bits flow to carry on the next iteration via >>= 8"
            )]
            {
                *byte = carry as u8;
            }
            carry >>= 8;
        }
        while carry > 0 {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "intentional low-byte extraction; high bits continue via the next loop iteration via >>= 8"
            )]
            bytes.push(carry as u8);
            carry >>= 8;
        }
    }

    // Reverse (we built it in little-endian order).
    bytes.reverse();

    // Prepend leading zeros.
    let mut result = vec![0u8; leading_zeros];
    result.extend_from_slice(&bytes);
    Ok(result)
}

// ---------------------------------------------------------------------------
// Hash helpers
// ---------------------------------------------------------------------------

/// HASH160: SHA-256 then RIPEMD-160 (same as Bitcoin).
fn hash160(data: &[u8]) -> [u8; 20] {
    let sha = Sha256::digest(data);
    let ripemd = Ripemd160::digest(sha);
    let mut out = [0u8; 20];
    out.copy_from_slice(&ripemd);
    out
}

/// Derive the 33-byte compressed public key from a private key.
///
/// # Errors
/// Returns `DontYeetWalletError::Crypto` if the key bytes are invalid.
pub(crate) fn compressed_pubkey(private_key: &PrivateKey) -> Result<Vec<u8>> {
    let signing_key = SigningKey::from_slice(private_key.as_bytes())
        .map_err(|e| DontYeetWalletError::Crypto(format!("invalid secp256k1 key: {e}")))?;
    let verifying_key = signing_key.verifying_key();
    let point = verifying_key.to_encoded_point(true);
    Ok(point.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compressed_pubkey_is_33_bytes() {
        let key_bytes =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .expect("valid hex");
        let pk = PrivateKey::new(key_bytes);
        let compressed = compressed_pubkey(&pk).expect("derive pubkey");
        assert_eq!(compressed.len(), 33);
    }

    #[test]
    fn known_key_produces_r_prefixed_address() {
        // Private key = 1 => known compressed public key.
        let key_bytes =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .expect("valid hex");
        let pk = PrivateKey::new(key_bytes);
        let pub_key = compressed_pubkey(&pk).expect("pubkey");

        let encoder = XrpAddressEncoder;
        let network = NetworkId::new("xrp-mainnet");
        let addr = encoder.encode(&pub_key, &network).expect("encode");

        // Must start with 'r'.
        assert!(addr.as_str().starts_with('r'));
        // Must be 25-35 characters.
        assert!(addr.as_str().len() >= 25);
        assert!(addr.as_str().len() <= 35);
        // Must validate successfully.
        encoder.validate(addr.as_str(), &network).expect("valid");
    }

    #[test]
    fn validate_valid_address() {
        let encoder = XrpAddressEncoder;
        let network = NetworkId::new("xrp-mainnet");
        // Generate an address from a known key to validate.
        let key_bytes =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000002")
                .expect("valid hex");
        let pk = PrivateKey::new(key_bytes);
        let pub_key = compressed_pubkey(&pk).expect("pubkey");
        let addr = encoder.encode(&pub_key, &network).expect("encode");
        assert!(encoder.validate(addr.as_str(), &network).is_ok());
    }

    #[test]
    fn validate_invalid_prefix_fails() {
        let encoder = XrpAddressEncoder;
        let network = NetworkId::new("xrp-mainnet");
        assert!(
            encoder
                .validate("Xnotanaddressatall12345", &network)
                .is_err()
        );
    }

    #[test]
    fn validate_too_short_fails() {
        let encoder = XrpAddressEncoder;
        let network = NetworkId::new("xrp-mainnet");
        assert!(encoder.validate("r1234", &network).is_err());
    }

    #[test]
    fn validate_bad_checksum_fails() {
        let encoder = XrpAddressEncoder;
        let network = NetworkId::new("xrp-mainnet");
        // Generate a valid address then mutate last char.
        let key_bytes =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .expect("valid hex");
        let pk = PrivateKey::new(key_bytes);
        let pub_key = compressed_pubkey(&pk).expect("pubkey");
        let addr = encoder.encode(&pub_key, &network).expect("encode");
        let addr_str = addr.as_str();
        // Flip the last character.
        let last = addr_str.chars().last().expect("non-empty");
        let replacement = if last == 'r' { 'p' } else { 'r' };
        let mut bad = addr_str[..addr_str.len() - 1].to_string();
        bad.push(replacement);
        assert!(encoder.validate(&bad, &network).is_err());
    }

    #[test]
    fn base58_roundtrip() {
        let original = vec![0x00, 0x01, 0x02, 0xFF, 0xFE];
        let encoded = xrp_base58_encode(&original);
        let decoded = xrp_base58_decode(&encoded).expect("decode");
        assert_eq!(decoded, original);
    }

    #[test]
    fn key_derivation_produces_32_bytes() {
        use dontyeet_crypto::mnemonic::{Bip39Generator, WordCount};

        let m = Bip39Generator::generate(WordCount::Twelve).expect("mnemonic");
        let seed = Bip39Generator::to_seed(&m, "").expect("seed");
        let pk =
            Bip44Deriver::derive(&seed, dontyeet_crypto::derivation::paths::XRP).expect("derive");
        assert_eq!(pk.as_bytes().len(), 32);
    }
}

// Rust guideline compliant 2026-05-02
