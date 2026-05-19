//! Blockchain network metadata.

use serde::{Deserialize, Serialize};

use crate::chain::{ChainId, NetworkCategory, NetworkId};

/// A specific blockchain network (e.g. Ethereum Mainnet, Polygon Amoy).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainNetwork {
    /// Unique identifier, e.g. `"ethereum-mainnet"`.
    pub id: NetworkId,
    /// Human-readable label, e.g. `"Ethereum Mainnet"`.
    pub label: String,
    /// Which chain this network belongs to.
    pub chain_id: ChainId,
    /// Mainnet, testnet, or devnet.
    pub category: NetworkCategory,
    /// EVM chain ID (if applicable). `None` for non-EVM chains.
    pub evm_chain_id: Option<u64>,
}

/// Block explorer URL templates for a network.
///
/// Templates use `{address}` and `{tx}` as placeholders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorerUrls {
    /// Template for viewing an address, e.g. `"https://etherscan.io/address/{address}"`.
    pub address_url: String,
    /// Template for viewing a transaction, e.g. `"https://etherscan.io/tx/{tx}"`.
    pub tx_url: String,
}

impl ExplorerUrls {
    /// Create a new set of explorer URL templates.
    #[must_use]
    pub fn new(address_url: impl Into<String>, tx_url: impl Into<String>) -> Self {
        Self {
            address_url: address_url.into(),
            tx_url: tx_url.into(),
        }
    }

    /// Format an address URL.
    #[must_use]
    pub fn format_address(&self, address: &str) -> String {
        self.address_url.replace("{address}", address)
    }

    /// Format a transaction URL.
    #[must_use]
    pub fn format_tx(&self, tx_hash: &str) -> String {
        self.tx_url.replace("{tx}", tx_hash)
    }
}

// Rust guideline compliant 2026-05-02
