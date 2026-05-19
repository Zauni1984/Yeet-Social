//! Bitcoin-specific error types for `DontYeetWallet`.

/// Errors originating from Bitcoin chain operations.
#[derive(Debug, thiserror::Error)]
pub enum BtcError {
    /// Key derivation or keypair construction failed.
    #[error("btc key derivation: {0}")]
    KeyDerivation(String),

    /// Address encoding, decoding, or validation failed.
    #[error("btc invalid address: {0}")]
    InvalidAddress(String),

    /// Transaction building or serialization failed.
    #[error("btc transaction build: {0}")]
    TransactionBuild(String),

    /// Transaction signing failed.
    #[error("btc signing: {0}")]
    Signing(String),

    /// Network or API call failed.
    #[error("btc network: {0}")]
    Network(String),
}

/// Convenience alias for Bitcoin operations.
pub type BtcResult<T> = std::result::Result<T, BtcError>;

impl From<BtcError> for dontyeet_primitives::DontYeetWalletError {
    fn from(e: BtcError) -> Self {
        Self::Chain(e.to_string())
    }
}

// Rust guideline compliant 2026-05-02
