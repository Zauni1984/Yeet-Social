//! Kadena-specific error types for `DontYeetWallet`.

/// Errors originating from Kadena chain operations.
#[derive(Debug, thiserror::Error)]
pub enum KadenaError {
    /// Key derivation or signing failed.
    #[error("kadena key derivation error: {0}")]
    KeyDerivation(String),

    /// Address encoding or validation failed.
    #[error("kadena address error: {0}")]
    InvalidAddress(String),

    /// Transaction building or encoding failed.
    #[error("kadena transaction error: {0}")]
    TransactionBuild(String),

    /// Signing operation failed.
    #[error("kadena signing error: {0}")]
    Signing(String),

    /// Network or API communication failed.
    #[error("kadena network error: {0}")]
    Network(String),
}

/// Convenience alias for Kadena operations.
pub type KadenaResult<T> = std::result::Result<T, KadenaError>;

impl From<KadenaError> for dontyeet_primitives::DontYeetWalletError {
    fn from(e: KadenaError) -> Self {
        Self::Chain(e.to_string())
    }
}

// Rust guideline compliant 2026-05-02
