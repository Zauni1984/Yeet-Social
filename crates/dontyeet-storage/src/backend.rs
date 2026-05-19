//! Backend trait for raw key-value storage.
//!
//! Implementations live in the app layer (filesystem, `SQLite`, browser
//! extension storage, etc.).  Library code depends only on this trait.

use async_trait::async_trait;

use crate::error::StorageResult;

/// A raw byte-level key-value store.
///
/// Implementations are injected at startup.  The storage layer never
/// knows (or cares) whether it's backed by a file, a database, or
/// browser `localStorage`.
#[async_trait]
pub trait KeyValueBackend: Send + Sync {
    /// Read the value for `key`, or `None` if it doesn't exist.
    ///
    /// # Errors
    /// Returns `StorageError::Backend` on I/O failure.
    async fn get(&self, key: &str) -> StorageResult<Option<Vec<u8>>>;

    /// Write `value` under `key`, overwriting any previous value.
    ///
    /// # Errors
    /// Returns `StorageError::Backend` on I/O failure.
    async fn set(&self, key: &str, value: &[u8]) -> StorageResult<()>;

    /// Delete `key`.  No-op if it doesn't exist.
    ///
    /// # Errors
    /// Returns `StorageError::Backend` on I/O failure.
    async fn delete(&self, key: &str) -> StorageResult<()>;

    /// List all keys currently stored.
    ///
    /// # Errors
    /// Returns `StorageError::Backend` on I/O failure.
    async fn list_keys(&self) -> StorageResult<Vec<String>>;

    /// Delete all keys.
    ///
    /// # Errors
    /// Returns `StorageError::Backend` on I/O failure.
    async fn clear(&self) -> StorageResult<()>;
}

// Rust guideline compliant 2026-05-02
