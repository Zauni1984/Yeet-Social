//! Encrypted data structures for storage.

use serde::{Deserialize, Serialize};

/// Identifies which encryption algorithm was used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CipherAlgorithm {
    /// AES-256-GCM authenticated encryption.
    Aes256Gcm,
    /// Hybrid ML-KEM-1024 + AES-256-GCM (post-quantum).
    HybridMlKemAes256Gcm,
}

/// A self-describing encrypted payload that can be stored and later decrypted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedPayload {
    /// Which algorithm produced this payload.
    pub algorithm: CipherAlgorithm,
    /// Nonce / IV bytes (12 bytes for AES-GCM).
    pub nonce: Vec<u8>,
    /// The ciphertext (includes GCM authentication tag).
    pub ciphertext: Vec<u8>,
    /// ML-KEM encapsulated key (only present for hybrid payloads).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kem_ciphertext: Option<Vec<u8>>,
}

// Rust guideline compliant 2026-05-02
