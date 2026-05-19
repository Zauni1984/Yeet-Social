//! EVM-specific error types for `DontYeetWallet`.

/// Errors originating from EVM chain operations.
#[derive(Debug, thiserror::Error)]
pub enum EvmError {
    /// Key derivation or signing failed.
    #[error("evm key error: {0}")]
    Key(String),

    /// Address encoding or validation failed.
    #[error("evm address error: {0}")]
    Address(String),

    /// Transaction building or encoding failed.
    #[error("evm transaction error: {0}")]
    Transaction(String),

    /// RPC call or network communication failed.
    #[error("evm rpc error: {0}")]
    Rpc(String),

    /// Hex decoding failed.
    #[error("evm hex error: {0}")]
    Hex(String),

    /// Fee estimation failed.
    #[error("evm fee error: {0}")]
    Fee(String),

    /// Configuration error.
    #[error("evm config error: {0}")]
    Config(String),
}

/// Convenience alias for EVM operations.
pub type EvmResult<T> = std::result::Result<T, EvmError>;

impl From<EvmError> for dontyeet_primitives::DontYeetWalletError {
    fn from(e: EvmError) -> Self {
        Self::Chain(e.to_string())
    }
}

// Rust guideline compliant 2026-05-02
