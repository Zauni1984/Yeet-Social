//! Transaction-related types.

use serde::{Deserialize, Serialize};

use crate::address::Address;
use crate::amount::Amount;

/// Parameters for a simple transfer (send native coin to an address).
///
/// Generic over fee type `F` because each chain uses a different fee model
/// (e.g. `EvmFees`, `BtcFees`, `SolanaFees`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleTxParams<F> {
    /// Recipient address.
    pub destination: Address,
    /// Amount to send (in smallest unit).
    pub amount: Amount,
    /// Chain-specific fee configuration.
    pub fees: F,
}

/// Three fee tiers for user selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeTier<F> {
    /// Slow / cheap option.
    pub slow: F,
    /// Standard / balanced option.
    pub standard: F,
    /// Fast / expensive option.
    pub fast: F,
}

/// Transaction hash returned after broadcast.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TxHash(String);

impl TxHash {
    /// Wrap a transaction hash string.
    #[must_use]
    pub fn new(hash: impl Into<String>) -> Self {
        Self(hash.into())
    }

    /// The raw hash string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TxHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A standardized, chain-agnostic fee display for a single tier.
///
/// Used by the API layer to present fee estimates without knowing
/// the chain-specific fee type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandardizedFee {
    /// Human-readable label, e.g. `"~0.0005 ETH"` or `"12 sat/vB"`.
    pub label: String,
    /// Estimated total fee in the native coin's smallest unit.
    pub native_amount: String,
}

/// Three standardized fee tiers returned by [`ChainPlugin::estimate_fee_display`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandardizedFeeTier {
    /// Slow / economy option.
    pub slow: StandardizedFee,
    /// Standard / balanced option.
    pub standard: StandardizedFee,
    /// Fast / priority option.
    pub fast: StandardizedFee,
}

/// Status of a broadcast transaction after confirmation polling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TxStatus {
    /// Transaction included in a block.
    Confirmed {
        /// Block number (chain-dependent representation).
        block_number: u64,
    },
    /// Transaction was included but execution reverted.
    Failed {
        /// Reason for failure, if available.
        reason: String,
    },
    /// Polling timed out without a receipt.
    Timeout,
}

// ---------------------------------------------------------------------------
// Transaction history types
// ---------------------------------------------------------------------------

/// Direction of a transaction relative to the queried address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TxDirection {
    /// Incoming — funds received.
    In,
    /// Outgoing — funds sent.
    Out,
    /// Direction is genuinely ambiguous (zero net delta, malformed RPC
    /// response, or a complex multi-leg tx where the queried address only
    /// appears via lookup tables). Frontends should render these neutrally
    /// rather than picking a side.
    Unknown,
}

impl std::fmt::Display for TxDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::In => write!(f, "in"),
            Self::Out => write!(f, "out"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Simplified confirmation status for history display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TxConfirmation {
    /// Included in a block.
    Confirmed,
    /// Broadcast but not yet confirmed.
    Pending,
    /// Reverted or rejected.
    Failed,
}

impl std::fmt::Display for TxConfirmation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Confirmed => write!(f, "confirmed"),
            Self::Pending => write!(f, "pending"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

/// A single transaction history entry, chain-agnostic.
///
/// Returned by chain indexer implementations and serialized
/// for the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxHistoryItem {
    /// Transaction hash.
    pub tx_hash: TxHash,
    /// Whether the queried address sent or received.
    pub direction: TxDirection,
    /// The other party's address.
    pub counterparty: Address,
    /// Transfer amount in the native coin's smallest unit.
    pub amount: Amount,
    /// Asset symbol (e.g. `"ETH"`, `"BTC"`).
    pub symbol: String,
    /// Unix timestamp in seconds, if known.
    pub timestamp: Option<i64>,
    /// Confirmation status.
    pub status: TxConfirmation,
}

// ---------------------------------------------------------------------------
// RPC health check
// ---------------------------------------------------------------------------

/// Result of pinging a chain's RPC endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcHealthResult {
    /// Whether the RPC responded successfully.
    pub reachable: bool,
    /// Round-trip time in milliseconds.
    pub response_time_ms: u64,
    /// Latest block or slot number, if available.
    pub latest_block: Option<u64>,
    /// Error message if the check failed.
    pub error: Option<String>,
}

// Rust guideline compliant 2026-05-02
