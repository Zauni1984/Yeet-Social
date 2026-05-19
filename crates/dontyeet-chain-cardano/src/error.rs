//! Cardano-specific error types for `DontYeetWallet`.

/// Errors originating from Cardano chain operations.
#[derive(Debug, thiserror::Error)]
pub enum CardanoError {
    /// Key derivation or keypair construction failed.
    #[error("cardano key derivation: {0}")]
    KeyDerivation(String),

    /// Address encoding, decoding, or validation failed.
    #[error("cardano invalid address: {0}")]
    InvalidAddress(String),

    /// Transaction building or serialization failed.
    #[error("cardano transaction build: {0}")]
    TransactionBuild(String),

    /// Transaction signing failed.
    #[error("cardano signing: {0}")]
    Signing(String),

    /// Network or API call failed.
    #[error("cardano network: {0}")]
    Network(String),
}

/// Convenience alias for Cardano operations.
pub type CardanoResult<T> = std::result::Result<T, CardanoError>;

impl From<CardanoError> for dontyeet_primitives::DontYeetWalletError {
    fn from(e: CardanoError) -> Self {
        Self::Chain(e.to_string())
    }
}

// Rust guideline compliant 2026-05-02
