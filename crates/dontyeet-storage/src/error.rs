//! Storage-specific error types.

/// Errors originating from storage operations.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// The underlying backend failed.
    #[error("backend: {0}")]
    Backend(String),

    /// Serialization or deserialization failed.
    #[error("serialization: {0}")]
    Serialization(String),

    /// Encryption or decryption of a stored value failed.
    #[error("encryption: {0}")]
    Encryption(String),

    /// The requested key was not found.
    #[error("key not found: {0}")]
    NotFound(String),

    /// Decrypted payload starts with a version byte this build does not recognize.
    ///
    /// Indicates either a forward-version blob written by a newer build or
    /// a corruption that survived the AES-GCM tag (which should not happen
    /// for a well-formed key). Surface to the user as "wallet from a newer
    /// version of the app — please upgrade."
    #[error("unknown payload version: {0:#04x}")]
    UnknownVersion(u8),
}

/// Convenience alias.
pub type StorageResult<T> = std::result::Result<T, StorageError>;

impl From<StorageError> for dontyeet_primitives::DontYeetWalletError {
    fn from(e: StorageError) -> Self {
        Self::Storage(e.to_string())
    }
}

// Rust guideline compliant 2026-05-02
