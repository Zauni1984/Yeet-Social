//! Kadena chain configuration.
//!
//! A single [`KadenaConfig`] struct parameterizes the Kadena plugin for
//! community mainnet and testnet (Chainweb `PoW`, 20 parallel chains).

use std::collections::HashMap;

use url::Url;

use dontyeet_primitives::asset::AssetInfo;
use dontyeet_primitives::chain::{ChainId, NetworkId};
use dontyeet_primitives::network::{BlockchainNetwork, ExplorerUrls};

/// Complete configuration for the Kadena Chainweb chain.
#[derive(Debug, Clone)]
pub struct KadenaConfig {
    /// Which chain this config represents (always [`ChainId::Kadena`]).
    pub chain_id: ChainId,
    /// Native asset metadata (KDA, 12 decimals).
    pub native_asset: AssetInfo,
    /// BIP-44 derivation path: `"m/44'/626'/0'/0/0"`.
    pub derivation_path: String,
    /// All networks (community mainnet, testnet) for Kadena.
    pub networks: Vec<BlockchainNetwork>,
    /// Pact API base URLs keyed by [`NetworkId`].
    ///
    /// These point to Chainweb node service APIs, e.g.
    /// `https://api.chainweb-community.org`.
    pub api_urls: HashMap<NetworkId, Vec<Url>>,
    /// Chainweb network version keyed by [`NetworkId`].
    ///
    /// e.g. `"mainnet01"` or `"testnet04"`.
    pub network_versions: HashMap<NetworkId, String>,
    /// Block explorer URL templates keyed by [`NetworkId`].
    pub explorer_urls: HashMap<NetworkId, ExplorerUrls>,
}

// Rust guideline compliant 2026-05-02
