#![deny(clippy::all, clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]

//! Offline cryptographic operations for `DontYeetWallet`.
//!
//! This is the **Core** layer — it depends only on `dontyeet-primitives`
//! and pure crypto crates.  It has **zero** network or I/O dependencies.
//!
//! ## Modules
//!
//! - [`mnemonic`] — BIP-39 generation, validation, seed derivation
//! - [`derivation`] — BIP-44 HD key derivation with well-known paths
//! - [`cipher`] — AES-256-GCM + hybrid ML-KEM post-quantum encryption
//! - [`hasher`] — Argon2id password hashing with constant-time verification
//! - [`payload`] — Encrypted data structures for storage

pub mod cipher;
pub mod derivation;
pub mod error;
pub mod hasher;
pub mod mnemonic;
pub mod payload;

pub use cipher::{AesGcmCipher, Cipher, HybridCipher};
pub use derivation::{Bip44Deriver, paths};
pub use error::{CryptoError, CryptoResult};
pub use hasher::{Argon2Config, Argon2Hasher, PasswordHasher};
pub use mnemonic::{Bip39Generator, WordCount};
pub use payload::{CipherAlgorithm, EncryptedPayload};

// Rust guideline compliant 2026-05-02
