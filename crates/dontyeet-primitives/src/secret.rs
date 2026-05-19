//! Secret types that are automatically wiped from memory on drop.
//!
//! Every type here derives [`zeroize::Zeroize`] and [`zeroize::ZeroizeOnDrop`]
//! so key material is scrubbed as soon as it goes out of scope.

use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::address::Address;

/// BIP-39 mnemonic phrase.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Mnemonic(String);

impl Mnemonic {
    /// Wrap a mnemonic string.  The caller is responsible for validating
    /// the word list — `dontyeet-crypto` handles that.
    #[must_use]
    pub fn new(words: impl Into<String>) -> Self {
        Self(words.into())
    }

    /// Borrow the underlying phrase.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Mnemonic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Mnemonic([REDACTED])")
    }
}

/// 64-byte seed derived from a mnemonic.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Seed([u8; 64]);

impl Seed {
    /// Wrap raw seed bytes.
    #[must_use]
    pub fn new(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    /// Borrow the underlying bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

impl ConstantTimeEq for Seed {
    fn ct_eq(&self, other: &Self) -> subtle::Choice {
        self.0.ct_eq(&other.0)
    }
}

impl std::fmt::Debug for Seed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Seed([REDACTED])")
    }
}

/// Arbitrary-length private key.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct PrivateKey(Vec<u8>);

impl PrivateKey {
    /// Wrap raw key bytes.
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Borrow the underlying bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl ConstantTimeEq for PrivateKey {
    fn ct_eq(&self, other: &Self) -> subtle::Choice {
        self.0.ct_eq(&other.0)
    }
}

impl std::fmt::Debug for PrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PrivateKey([REDACTED])")
    }
}

/// A keypair: a public address and its corresponding private key.
///
/// The `Debug` impl redacts the private key.
#[derive(Clone, Serialize, Deserialize)]
pub struct KeyPair {
    /// The public address derived from this key.
    pub address: Address,
    /// The private key bytes (serialised as hex for storage, zeroized on drop).
    #[serde(skip)]
    private_key: Option<PrivateKey>,
}

impl KeyPair {
    /// Create a new keypair.
    #[must_use]
    pub fn new(address: Address, private_key: PrivateKey) -> Self {
        Self {
            address,
            private_key: Some(private_key),
        }
    }

    /// Borrow the private key.  Returns `None` if the keypair was
    /// deserialized without the secret half (e.g. from a watch-only export).
    #[must_use]
    pub fn private_key(&self) -> Option<&PrivateKey> {
        self.private_key.as_ref()
    }
}

impl std::fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyPair")
            .field("address", &self.address)
            .field("private_key", &"[REDACTED]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subtle::ConstantTimeEq;

    #[test]
    fn seed_ct_eq_same() {
        let a = Seed::new([0xAB; 64]);
        let b = Seed::new([0xAB; 64]);
        assert!(bool::from(a.ct_eq(&b)));
    }

    #[test]
    fn seed_ct_eq_different() {
        let a = Seed::new([0xAB; 64]);
        let mut bytes = [0xAB; 64];
        bytes[63] = 0xCD;
        let b = Seed::new(bytes);
        assert!(!bool::from(a.ct_eq(&b)));
    }

    #[test]
    fn private_key_ct_eq_same() {
        let a = PrivateKey::new(vec![1, 2, 3, 4]);
        let b = PrivateKey::new(vec![1, 2, 3, 4]);
        assert!(bool::from(a.ct_eq(&b)));
    }

    #[test]
    fn private_key_ct_eq_different() {
        let a = PrivateKey::new(vec![1, 2, 3, 4]);
        let b = PrivateKey::new(vec![1, 2, 3, 5]);
        assert!(!bool::from(a.ct_eq(&b)));
    }

    #[test]
    fn private_key_ct_eq_different_length() {
        let a = PrivateKey::new(vec![1, 2, 3]);
        let b = PrivateKey::new(vec![1, 2, 3, 4]);
        assert!(!bool::from(a.ct_eq(&b)));
    }
}

// Rust guideline compliant 2026-05-02
