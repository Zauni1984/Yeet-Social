//! Blockchain address type.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A blockchain address (hex, base58, bech32, etc.).
///
/// This is a thin wrapper — validation is chain-specific and happens in the
/// [`AddressEncoder`](crate::traits::AddressEncoder) implementations.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Address(String);

impl Address {
    /// Wrap a validated address string.
    #[must_use]
    pub fn new(addr: impl Into<String>) -> Self {
        Self(addr.into())
    }

    /// The raw address string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for Address {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

// Rust guideline compliant 2026-05-02
