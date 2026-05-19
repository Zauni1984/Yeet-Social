#![deny(clippy::all, clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]

//! Encrypted key-value storage abstraction for `DontYeetWallet`.
//!
//! This is the **Core** layer — it depends on `dontyeet-primitives` and
//! `dontyeet-crypto`.  Storage backends are injected at runtime.
//!
//! ## Modules
//!
//! - [`backend`] — `KeyValueBackend` trait (raw bytes, backend-agnostic)
//! - [`encrypted`] — `EncryptedStore` wrapping backend + cipher
//! - [`serializer`] — `Serializer` trait + `JsonSerializer`

pub mod backend;
pub mod encrypted;
pub mod error;
pub mod serializer;

pub use backend::KeyValueBackend;
pub use encrypted::{CURRENT_VERSION, EncryptedStore, UNKNOWN_VERSION};
pub use error::{StorageError, StorageResult};
pub use serializer::{JsonSerializer, Serializer};

// Rust guideline compliant 2026-05-02
