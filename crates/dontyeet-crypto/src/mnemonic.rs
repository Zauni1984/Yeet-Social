//! BIP-39 mnemonic generation, validation, and seed derivation.

use bip39::Language;
use dontyeet_primitives::{Mnemonic, Seed};
use zeroize::Zeroize;

use crate::error::{CryptoError, CryptoResult};

/// Number of words in a generated mnemonic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordCount {
    /// 12 words (128 bits of entropy).
    Twelve,
    /// 24 words (256 bits of entropy, recommended).
    TwentyFour,
}

/// BIP-39 mnemonic operations.
pub struct Bip39Generator;

impl Bip39Generator {
    /// Generate a new random BIP-39 mnemonic.
    ///
    /// # Errors
    /// Returns `CryptoError::Mnemonic` if generation fails.
    pub fn generate(word_count: WordCount) -> CryptoResult<Mnemonic> {
        let entropy_bytes = match word_count {
            WordCount::Twelve => 16,     // 128 bits
            WordCount::TwentyFour => 32, // 256 bits
        };

        let mut entropy = vec![0u8; entropy_bytes];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut entropy);

        let m = bip39::Mnemonic::from_entropy(&entropy)
            .map_err(|e| CryptoError::Mnemonic(format!("generation failed: {e}")))?;
        entropy.zeroize();

        Ok(Mnemonic::new(m.to_string()))
    }

    /// Validate that a mnemonic phrase is well-formed BIP-39.
    ///
    /// # Errors
    /// Returns `CryptoError::Mnemonic` if the phrase is invalid.
    pub fn validate(phrase: &str) -> CryptoResult<()> {
        bip39::Mnemonic::parse_in(Language::English, phrase)
            .map_err(|e| CryptoError::Mnemonic(format!("invalid mnemonic: {e}")))?;
        Ok(())
    }

    /// Derive a 64-byte seed from a mnemonic, optionally with a passphrase.
    ///
    /// The passphrase provides an additional layer of protection — a different
    /// passphrase produces a completely different seed from the same mnemonic.
    ///
    /// # Errors
    /// Returns `CryptoError::Mnemonic` if the mnemonic is invalid.
    pub fn to_seed(mnemonic: &Mnemonic, passphrase: &str) -> CryptoResult<Seed> {
        let m = bip39::Mnemonic::parse_in(Language::English, mnemonic.as_str())
            .map_err(|e| CryptoError::Mnemonic(format!("invalid mnemonic: {e}")))?;

        let mut seed_bytes = m.to_seed(passphrase);
        let seed = Seed::new(seed_bytes);
        seed_bytes.zeroize();
        Ok(seed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_12_words() {
        let m = Bip39Generator::generate(WordCount::Twelve).expect("should generate");
        assert_eq!(m.as_str().split_whitespace().count(), 12);
    }

    #[test]
    fn generate_24_words() {
        let m = Bip39Generator::generate(WordCount::TwentyFour).expect("should generate");
        assert_eq!(m.as_str().split_whitespace().count(), 24);
    }

    #[test]
    fn validate_good_mnemonic() {
        let m = Bip39Generator::generate(WordCount::Twelve).expect("should generate");
        assert!(Bip39Generator::validate(m.as_str()).is_ok());
    }

    #[test]
    fn validate_bad_mnemonic() {
        assert!(Bip39Generator::validate("not a valid mnemonic phrase at all").is_err());
    }

    #[test]
    fn seed_derivation_deterministic() {
        let m = Bip39Generator::generate(WordCount::Twelve).expect("should generate");
        let s1 = Bip39Generator::to_seed(&m, "").expect("seed");
        let s2 = Bip39Generator::to_seed(&m, "").expect("seed");
        assert_eq!(s1.as_bytes(), s2.as_bytes());
    }

    #[test]
    fn different_passphrase_different_seed() {
        let m = Bip39Generator::generate(WordCount::Twelve).expect("should generate");
        let s1 = Bip39Generator::to_seed(&m, "").expect("seed");
        let s2 = Bip39Generator::to_seed(&m, "secret").expect("seed");
        assert_ne!(s1.as_bytes(), s2.as_bytes());
    }
}

// Rust guideline compliant 2026-05-02
