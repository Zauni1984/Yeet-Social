//! TRON-specific error types for `DontYeetWallet`.

/// Errors originating from TRON chain operations.
#[derive(Debug, thiserror::Error)]
pub enum TronError {
    /// Key derivation or keypair construction failed.
    #[error("tron key derivation: {0}")]
    KeyDerivation(String),

    /// Address encoding, decoding, or validation failed.
    #[error("tron invalid address: {0}")]
    InvalidAddress(String),

    /// Transaction building or serialization failed.
    #[error("tron transaction build: {0}")]
    TransactionBuild(String),

    /// Transaction signing failed.
    #[error("tron signing: {0}")]
    Signing(String),

    /// Network or API call failed.
    #[error("tron network: {0}")]
    Network(String),
}

/// Convenience alias for TRON operations.
pub type TronResult<T> = std::result::Result<T, TronError>;

impl From<TronError> for dontyeet_primitives::DontYeetWalletError {
    fn from(e: TronError) -> Self {
        Self::Chain(e.to_string())
    }
}

// Rust guideline compliant 2026-05-02
