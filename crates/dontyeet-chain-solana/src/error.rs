//! Solana-specific error types for `DontYeetWallet`.

/// Errors originating from Solana chain operations.
#[derive(Debug, thiserror::Error)]
pub enum SolError {
    /// Key derivation or keypair construction failed.
    #[error("sol key derivation: {0}")]
    KeyDerivation(String),

    /// Address encoding, decoding, or validation failed.
    #[error("sol invalid address: {0}")]
    InvalidAddress(String),

    /// Transaction building or serialization failed.
    #[error("sol transaction build: {0}")]
    TransactionBuild(String),

    /// Transaction signing failed.
    #[error("sol signing: {0}")]
    Signing(String),

    /// Network or API call failed.
    #[error("sol network: {0}")]
    Network(String),
}

/// Convenience alias for Solana operations.
pub type SolResult<T> = std::result::Result<T, SolError>;

impl From<SolError> for dontyeet_primitives::DontYeetWalletError {
    fn from(e: SolError) -> Self {
        Self::Chain(e.to_string())
    }
}

// Rust guideline compliant 2026-05-02
