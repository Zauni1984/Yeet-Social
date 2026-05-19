//! Wallet-specific error types, mapped to pipeline stages.

/// Errors originating from wallet operations.
#[derive(Debug, thiserror::Error)]
pub enum WalletError {
    /// Stage 1: destination address failed validation.
    #[error("invalid address: {0}")]
    InvalidAddress(String),

    /// Stage 2: balance too low for the requested transfer.
    #[error("insufficient funds: need {needed}, have {available}")]
    InsufficientFunds {
        /// Amount required.
        needed: String,
        /// Amount available.
        available: String,
    },

    /// Stage 2: keypair is watch-only (no private key).
    #[error("no private key — wallet is watch-only")]
    NoPrivateKey,

    /// The requested network is not supported by this chain plugin.
    #[error("unsupported network: {0}")]
    UnsupportedNetwork(String),

    /// Fee estimation failed.
    #[error("fee estimation: {0}")]
    FeeEstimation(String),

    /// Stage 3: transaction building failed.
    #[error("build failed: {0}")]
    BuildFailed(String),

    /// Stage 3: transaction signing failed.
    #[error("signing failed: {0}")]
    SigningFailed(String),

    /// Stage 4: transaction broadcast failed.
    #[error("broadcast failed: {0}")]
    BroadcastFailed(String),

    /// Passthrough from primitives.
    #[error(transparent)]
    Primitives(#[from] dontyeet_primitives::DontYeetWalletError),
}

/// Convenience alias.
pub type WalletResult<T> = std::result::Result<T, WalletError>;

impl From<WalletError> for dontyeet_primitives::DontYeetWalletError {
    fn from(e: WalletError) -> Self {
        match e {
            WalletError::InsufficientFunds { needed, available } => {
                Self::InsufficientFunds { needed, available }
            }
            WalletError::InvalidAddress(msg) => Self::Validation(msg),
            WalletError::Primitives(inner) => inner,
            other => Self::Chain(other.to_string()),
        }
    }
}

// Rust guideline compliant 2026-05-02
