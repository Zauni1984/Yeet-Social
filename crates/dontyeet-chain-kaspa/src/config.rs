//! Kaspa chain configuration.
//!
//! A single [`KaspaConfig`] struct parameterizes the Kaspa plugin for
//! mainnet and testnet (`BlockDAG` networks using the GHOSTDAG protocol).

use std::collections::HashMap;

use url::Url;

use dontyeet_primitives::asset::AssetInfo;
use dontyeet_primitives::chain::{ChainId, NetworkId};
use dontyeet_primitives::network::{BlockchainNetwork, ExplorerUrls};

/// Complete configuration for the Kaspa `BlockDAG` chain.
#[derive(Debug, Clone)]
pub struct KaspaConfig {
    /// Which chain this config represents (always [`ChainId::Kaspa`]).
    pub chain_id: ChainId,
    /// Native asset metadata (KAS, 8 decimals).
    pub native_asset: AssetInfo,
    /// BIP-44 derivation path: `"m/44'/111111'/0'/0/0"`.
    pub derivation_path: String,
    /// All networks (mainnet, testnet) for Kaspa.
    pub networks: Vec<BlockchainNetwork>,
    /// REST API base URLs keyed by [`NetworkId`].
    pub api_urls: HashMap<NetworkId, Vec<Url>>,
    /// Block explorer URL templates keyed by [`NetworkId`].
    pub explorer_urls: HashMap<NetworkId, ExplorerUrls>,
}

// Rust guideline compliant 2026-05-02
