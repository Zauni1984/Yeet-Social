//! Kaspa-specific error types for `DontYeetWallet`.

/// Errors originating from Kaspa chain operations.
#[derive(Debug, thiserror::Error)]
pub enum KaspaError {
    /// Key derivation or signing failed.
    #[error("kaspa key derivation error: {0}")]
    KeyDerivation(String),

    /// Address encoding or validation failed.
    #[error("kaspa address error: {0}")]
    InvalidAddress(String),

    /// Transaction building or encoding failed.
    #[error("kaspa transaction error: {0}")]
    TransactionBuild(String),

    /// Signing operation failed.
    #[error("kaspa signing error: {0}")]
    Signing(String),

    /// Network or API communication failed.
    #[error("kaspa network error: {0}")]
    Network(String),
}

/// Convenience alias for Kaspa operations.
pub type KaspaResult<T> = std::result::Result<T, KaspaError>;

impl From<KaspaError> for dontyeet_primitives::DontYeetWalletError {
    fn from(e: KaspaError) -> Self {
        Self::Chain(e.to_string())
    }
}

// Rust guideline compliant 2026-05-02
