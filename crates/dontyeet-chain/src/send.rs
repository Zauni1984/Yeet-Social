//! Transaction send result.

use dontyeet_primitives::TxHash;
use serde::{Deserialize, Serialize};

/// The result of a successful `send_simple` operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendResult {
    /// The transaction hash on-chain.
    pub tx_hash: TxHash,
    /// A fully-formed block explorer URL for this transaction.
    pub explorer_url: String,
}

// Rust guideline compliant 2026-05-02
