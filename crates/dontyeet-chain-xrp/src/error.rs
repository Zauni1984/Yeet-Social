//! XRP-specific error types for `DontYeetWallet`.

/// Errors originating from XRP Ledger chain operations.
#[derive(Debug, thiserror::Error)]
pub enum XrpError {
    /// Key derivation or keypair construction failed.
    #[error("xrp key derivation: {0}")]
    KeyDerivation(String),

    /// Address encoding, decoding, or validation failed.
    #[error("xrp invalid address: {0}")]
    InvalidAddress(String),

    /// Transaction building or serialization failed.
    #[error("xrp transaction build: {0}")]
    TransactionBuild(String),

    /// Transaction signing failed.
    #[error("xrp signing: {0}")]
    Signing(String),

    /// Network or API call failed.
    #[error("xrp network: {0}")]
    Network(String),
}

/// Convenience alias for XRP operations.
pub type XrpResult<T> = std::result::Result<T, XrpError>;

impl From<XrpError> for dontyeet_primitives::DontYeetWalletError {
    fn from(e: XrpError) -> Self {
        Self::Chain(e.to_string())
    }
}

// Rust guideline compliant 2026-05-02
