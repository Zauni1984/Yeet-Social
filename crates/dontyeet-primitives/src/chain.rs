//! Chain identifiers, network categories, and network IDs.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Identifies a blockchain.
///
/// Well-known chains have explicit variants.  Unknown or plugin-added chains
/// use the `Other(String)` variant so the set is extensible without modifying
/// this enum.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChainId {
    /// Ethereum mainnet and EVM-compatible L1.
    Ethereum,
    /// Polygon `PoS` (native token: POL, formerly MATIC).
    Polygon,
    /// BNB Smart Chain.
    Bnb,
    /// Avalanche C-Chain.
    Avalanche,
    /// Sonic (formerly Fantom; native token: S).
    Sonic,
    /// Bitcoin.
    Bitcoin,
    /// Solana.
    Solana,
    /// Cardano.
    Cardano,
    /// XRP Ledger.
    Xrp,
    /// Algorand.
    Algorand,
    /// TRON.
    Tron,
    /// Kaspa (`BlockDAG`, GHOSTDAG protocol).
    Kaspa,
    /// Kadena (Chainweb `PoW`, community edition).
    Kadena,
    /// Plugin-provided chain not known at compile time.
    Other(String),
}

impl fmt::Display for ChainId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ethereum => write!(f, "ethereum"),
            Self::Polygon => write!(f, "polygon"),
            Self::Bnb => write!(f, "bnb"),
            Self::Avalanche => write!(f, "avalanche"),
            Self::Sonic => write!(f, "sonic"),
            Self::Bitcoin => write!(f, "bitcoin"),
            Self::Solana => write!(f, "solana"),
            Self::Cardano => write!(f, "cardano"),
            Self::Xrp => write!(f, "xrp"),
            Self::Algorand => write!(f, "algorand"),
            Self::Tron => write!(f, "tron"),
            Self::Kaspa => write!(f, "kaspa"),
            Self::Kadena => write!(f, "kadena"),
            Self::Other(id) => write!(f, "{id}"),
        }
    }
}

/// Mainnet vs testnet classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkCategory {
    /// Production network.
    Mainnet,
    /// Test network (tokens have no real value).
    Testnet,
    /// Development network (local or short-lived).
    Devnet,
}

impl fmt::Display for NetworkCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mainnet => write!(f, "mainnet"),
            Self::Testnet => write!(f, "testnet"),
            Self::Devnet => write!(f, "devnet"),
        }
    }
}

/// Unique identifier for a specific network within a chain.
///
/// Convention: `"<chain>-<network>"`, e.g. `"ethereum-mainnet"`,
/// `"polygon-amoy"`, `"solana-devnet"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NetworkId(pub String);

impl NetworkId {
    /// Create a new network ID.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl fmt::Display for NetworkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for NetworkId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

// Rust guideline compliant 2026-05-02
