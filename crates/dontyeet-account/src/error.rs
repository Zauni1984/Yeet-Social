//! Account-specific error types.

/// Errors originating from account operations.
#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    /// Tried to access a resource that requires authentication.
    #[error("not authenticated — login required")]
    NotAuthenticated,

    /// Tried to create an account when one already exists.
    #[error("account already exists")]
    AlreadyExists,

    /// Tried to login or delete when no account exists.
    #[error("no account found")]
    NotFound,

    /// Password verification failed.
    #[error("wrong password")]
    WrongPassword,

    /// Underlying storage error.
    #[error("storage: {0}")]
    Storage(String),

    /// Underlying crypto error (encryption, hashing).
    #[error("crypto: {0}")]
    Crypto(String),
}

/// Convenience alias.
pub type AccountResult<T> = std::result::Result<T, AccountError>;

impl From<dontyeet_storage::StorageError> for AccountError {
    fn from(e: dontyeet_storage::StorageError) -> Self {
        Self::Storage(e.to_string())
    }
}

impl From<dontyeet_crypto::CryptoError> for AccountError {
    fn from(e: dontyeet_crypto::CryptoError) -> Self {
        Self::Crypto(e.to_string())
    }
}

impl From<AccountError> for dontyeet_primitives::DontYeetWalletError {
    fn from(e: AccountError) -> Self {
        match e {
            AccountError::NotAuthenticated => Self::NotAuthenticated,
            // Map both NotFound and WrongPassword to the same public error
            // to prevent account enumeration via error differentiation.
            AccountError::NotFound | AccountError::WrongPassword => {
                Self::NotFound("account".into())
            }
            other => Self::Chain(other.to_string()),
        }
    }
}

// Rust guideline compliant 2026-05-02
