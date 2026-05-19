//! Algorand-specific error types for `DontYeetWallet`.

/// Errors originating from Algorand chain operations.
#[derive(Debug, thiserror::Error)]
pub enum AlgoError {
    /// Key derivation or keypair construction failed.
    #[error("algo key derivation: {0}")]
    KeyDerivation(String),

    /// Address encoding, decoding, or validation failed.
    #[error("algo invalid address: {0}")]
    InvalidAddress(String),

    /// Transaction building or serialization failed.
    #[error("algo transaction build: {0}")]
    TransactionBuild(String),

    /// Transaction signing failed.
    #[error("algo signing: {0}")]
    Signing(String),

    /// Network or API call failed.
    #[error("algo network: {0}")]
    Network(String),
}

/// Convenience alias for Algorand operations.
pub type AlgoResult<T> = std::result::Result<T, AlgoError>;

impl From<AlgoError> for dontyeet_primitives::DontYeetWalletError {
    fn from(e: AlgoError) -> Self {
        Self::Chain(e.to_string())
    }
}

// Rust guideline compliant 2026-05-02
