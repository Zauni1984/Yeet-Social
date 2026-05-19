//! Identity resolution traits.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::address::Address;
use crate::chain::ChainId;
use crate::error::Result;

/// A resolved mapping from a human identifier to a blockchain address.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityRecord {
    /// The original identifier (email, phone, handle, etc.).
    pub identifier: String,
    /// Name of the provider that resolved it.
    pub provider: String,
    /// Which chain this address is on.
    pub chain_id: ChainId,
    /// The resolved blockchain address.
    pub address: Address,
}

/// Resolve a human-readable identifier to one or more blockchain addresses.
#[async_trait]
pub trait IdentityResolver: Send + Sync {
    /// Attempt to resolve the identifier.
    ///
    /// Returns `Ok(vec![])` if the identifier is syntactically valid for this
    /// provider but has no registered address.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError` if resolution fails.
    async fn resolve(&self, identifier: &str) -> Result<Vec<IdentityRecord>>;

    /// Whether this resolver can handle the given identifier pattern
    /// (e.g. email format, phone format, `@handle`).
    fn can_handle(&self, identifier: &str) -> bool;
}

// Rust guideline compliant 2026-05-02
