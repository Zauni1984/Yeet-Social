//! Crypto-specific error types.

/// Errors originating from cryptographic operations.
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    /// Mnemonic generation, parsing, or seed derivation failed.
    #[error("mnemonic: {0}")]
    Mnemonic(String),

    /// HD key derivation failed.
    #[error("derivation: {0}")]
    Derivation(String),

    /// Encryption or decryption failed.
    #[error("cipher: {0}")]
    Cipher(String),

    /// Password hashing or verification failed.
    #[error("hasher: {0}")]
    Hasher(String),

    /// ML-KEM (post-quantum) operation failed.
    #[error("pqc: {0}")]
    PostQuantum(String),
}

/// Convenience alias.
pub type CryptoResult<T> = std::result::Result<T, CryptoError>;

impl From<CryptoError> for dontyeet_primitives::DontYeetWalletError {
    fn from(e: CryptoError) -> Self {
        Self::Crypto(e.to_string())
    }
}

// Rust guideline compliant 2026-05-02
