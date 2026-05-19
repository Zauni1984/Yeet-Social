//! EVM chain configuration.
//!
//! A single [`EvmChainConfig`] struct parameterizes the entire EVM plugin
//! for any EVM-compatible chain (Ethereum, Polygon, BNB, Avalanche, Sonic).

use std::collections::HashMap;

use url::Url;

use dontyeet_primitives::asset::AssetInfo;
use dontyeet_primitives::chain::{ChainId, NetworkId};
use dontyeet_primitives::network::{BlockchainNetwork, ExplorerUrls};

/// Complete configuration for one EVM-compatible chain.
///
/// The same [`EvmChainPlugin`](crate::plugin::EvmChainPlugin) struct is
/// reused across Ethereum, Polygon, BNB, etc. — only the config differs.
#[derive(Debug, Clone)]
pub struct EvmChainConfig {
    /// Which chain this config represents.
    pub chain_id: ChainId,
    /// Native asset metadata (name, symbol, decimals).
    pub native_asset: AssetInfo,
    /// BIP-44 derivation path, e.g. `"m/44'/60'/0'/0/0"`.
    pub derivation_path: String,
    /// EVM numeric chain ID for the mainnet network.
    pub evm_chain_id_mainnet: u64,
    /// All networks (mainnet, testnets) for this chain.
    pub networks: Vec<BlockchainNetwork>,
    /// RPC endpoint URLs keyed by [`NetworkId`].
    pub rpc_urls: HashMap<NetworkId, Vec<Url>>,
    /// Block explorer URL templates keyed by [`NetworkId`].
    pub explorer_urls: HashMap<NetworkId, ExplorerUrls>,
    /// Etherscan-compatible API base URLs for tx history, keyed by [`NetworkId`].
    ///
    /// Not all EVM chains have free explorer APIs. Missing entries are fine —
    /// the history fetcher gracefully returns `Unsupported`.
    pub explorer_api_urls: HashMap<NetworkId, Url>,
    /// Optional API key for Etherscan-compatible endpoints (free tier works
    /// without one, but at lower rate limits).
    pub explorer_api_key: Option<String>,
}

// Rust guideline compliant 2026-05-02
