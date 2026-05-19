//! Top-level error types for `DontYeetWallet`.

/// Top-level error enum for the `DontYeetWallet` workspace.
///
/// Each domain maps to a variant so callers can match broadly
/// without importing crate-specific error types.
#[derive(Debug, thiserror::Error)]
pub enum DontYeetWalletError {
    /// Cryptographic operation failed (key derivation, signing, encryption).
    #[error("crypto: {0}")]
    Crypto(String),

    /// Network or RPC call failed.
    #[error("network: {0}")]
    Network(String),

    /// Storage read/write failed.
    #[error("storage: {0}")]
    Storage(String),

    /// Chain-specific operation failed.
    #[error("chain: {0}")]
    Chain(String),

    /// Identity resolution failed.
    #[error("identity: {0}")]
    Identity(String),

    /// Input validation failed (address format, amount range, etc.).
    #[error("validation: {0}")]
    Validation(String),

    /// Insufficient funds for the requested operation.
    ///
    /// The `Display` impl intentionally hides the amounts to prevent
    /// balance enumeration via the API.  The UI layer can read `needed`
    /// and `available` directly from the variant to show the user what
    /// they're missing.
    #[error("insufficient funds for this transaction")]
    InsufficientFunds {
        /// Amount required.
        needed: String,
        /// Amount available.
        available: String,
    },

    /// Account is not logged in.
    #[error("not authenticated")]
    NotAuthenticated,

    /// Requested resource was not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// Operation is not supported for this chain or provider.
    #[error("unsupported: {0}")]
    Unsupported(String),
}

/// Convenience alias used throughout the workspace.
pub type Result<T> = std::result::Result<T, DontYeetWalletError>;

// Rust guideline compliant 2026-05-02
