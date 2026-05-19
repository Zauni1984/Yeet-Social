//! BIP-44 HD key derivation from seed + derivation path.

use bip32::{DerivationPath, XPrv};
use dontyeet_primitives::{PrivateKey, Seed};
use zeroize::Zeroize;

use crate::error::{CryptoError, CryptoResult};

/// BIP-44 hierarchical deterministic key derivation.
pub struct Bip44Deriver;

impl Bip44Deriver {
    /// Derive a private key from a seed using the given BIP-44 derivation path.
    ///
    /// # Arguments
    /// * `seed` — 64-byte master seed (from BIP-39).
    /// * `path` — Derivation path string, e.g. `"m/44'/60'/0'/0/0"` for Ethereum.
    ///
    /// # Errors
    /// Returns `CryptoError::Derivation` if the path is invalid or derivation fails.
    pub fn derive(seed: &Seed, path: &str) -> CryptoResult<PrivateKey> {
        let derivation_path: DerivationPath = path
            .parse()
            .map_err(|e| CryptoError::Derivation(format!("invalid path '{path}': {e}")))?;

        let xprv = XPrv::derive_from_path(seed.as_bytes(), &derivation_path)
            .map_err(|e| CryptoError::Derivation(format!("derivation failed: {e}")))?;

        let mut key_bytes = xprv.to_bytes();
        let private_key = PrivateKey::new(key_bytes.to_vec());
        key_bytes.zeroize();

        Ok(private_key)
    }
}

/// Well-known BIP-44 derivation paths (April 2026).
pub mod paths {
    /// Ethereum and EVM-compatible chains: `m/44'/60'/0'/0/0`
    pub const ETHEREUM: &str = "m/44'/60'/0'/0/0";
    /// Polygon: `m/44'/966'/0'/0/0`
    pub const POLYGON: &str = "m/44'/966'/0'/0/0";
    /// BNB Chain: `m/44'/714'/0'/0/0`
    pub const BNB: &str = "m/44'/714'/0'/0/0";
    /// Avalanche C-Chain: `m/44'/9005'/0'/0/0`
    pub const AVALANCHE: &str = "m/44'/9005'/0'/0/0";
    /// Sonic (formerly Fantom): `m/44'/1007'/0'/0/0`
    pub const SONIC: &str = "m/44'/1007'/0'/0/0";
    /// Bitcoin (legacy P2PKH): `m/44'/0'/0'/0/0`
    pub const BITCOIN_LEGACY: &str = "m/44'/0'/0'/0/0";
    /// Bitcoin (segwit P2WPKH): `m/84'/0'/0'/0/0`
    pub const BITCOIN_SEGWIT: &str = "m/84'/0'/0'/0/0";
    /// Solana: `m/44'/501'/0'/0'`
    pub const SOLANA: &str = "m/44'/501'/0'/0'";
    /// XRP Ledger: `m/44'/144'/0'/0/0`
    pub const XRP: &str = "m/44'/144'/0'/0/0";
    /// Algorand: `m/44'/283'/0'/0/0`
    pub const ALGORAND: &str = "m/44'/283'/0'/0/0";
    /// TRON: `m/44'/195'/0'/0/0`
    pub const TRON: &str = "m/44'/195'/0'/0/0";
    /// Cardano (CIP-1852): `m/1852'/1815'/0'/0/0`
    pub const CARDANO: &str = "m/1852'/1815'/0'/0/0";
    /// Kaspa: `m/44'/111111'/0'/0/0`
    pub const KASPA: &str = "m/44'/111111'/0'/0/0";
    /// Kadena (SLIP-44 coin type 626): `m/44'/626'/0'/0/0`
    pub const KADENA: &str = "m/44'/626'/0'/0/0";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mnemonic::{Bip39Generator, WordCount};

    #[test]
    fn derive_ethereum_key() {
        let m = Bip39Generator::generate(WordCount::Twelve).expect("mnemonic");
        let seed = Bip39Generator::to_seed(&m, "").expect("seed");
        let key = Bip44Deriver::derive(&seed, paths::ETHEREUM).expect("derive");
        assert_eq!(key.as_bytes().len(), 32);
    }

    #[test]
    fn derive_bitcoin_segwit_key() {
        let m = Bip39Generator::generate(WordCount::TwentyFour).expect("mnemonic");
        let seed = Bip39Generator::to_seed(&m, "").expect("seed");
        let key = Bip44Deriver::derive(&seed, paths::BITCOIN_SEGWIT).expect("derive");
        assert_eq!(key.as_bytes().len(), 32);
    }

    #[test]
    fn same_seed_same_key() {
        let m = Bip39Generator::generate(WordCount::Twelve).expect("mnemonic");
        let seed = Bip39Generator::to_seed(&m, "").expect("seed");
        let k1 = Bip44Deriver::derive(&seed, paths::ETHEREUM).expect("derive");
        let k2 = Bip44Deriver::derive(&seed, paths::ETHEREUM).expect("derive");
        assert_eq!(k1.as_bytes(), k2.as_bytes());
    }

    #[test]
    fn different_paths_different_keys() {
        let m = Bip39Generator::generate(WordCount::Twelve).expect("mnemonic");
        let seed = Bip39Generator::to_seed(&m, "").expect("seed");
        let k1 = Bip44Deriver::derive(&seed, paths::ETHEREUM).expect("derive");
        let k2 = Bip44Deriver::derive(&seed, paths::BITCOIN_LEGACY).expect("derive");
        assert_ne!(k1.as_bytes(), k2.as_bytes());
    }

    #[test]
    fn invalid_path_errors() {
        let m = Bip39Generator::generate(WordCount::Twelve).expect("mnemonic");
        let seed = Bip39Generator::to_seed(&m, "").expect("seed");
        assert!(Bip44Deriver::derive(&seed, "not/a/path").is_err());
    }
}

// Rust guideline compliant 2026-05-02
