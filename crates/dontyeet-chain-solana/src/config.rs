//! Solana chain configuration.
//!
//! Parameterizes the Solana plugin with mainnet, devnet, and testnet
//! network metadata, API URLs, and explorer templates.

use std::collections::HashMap;

use url::Url;

use dontyeet_primitives::asset::AssetInfo;
use dontyeet_primitives::chain::{ChainId, NetworkCategory, NetworkId};
use dontyeet_primitives::network::{BlockchainNetwork, ExplorerUrls};

/// Complete configuration for the Solana chain plugin.
///
/// Covers mainnet-beta, devnet, and testnet networks. API endpoints
/// point to Solana JSON-RPC nodes.
#[derive(Debug, Clone)]
pub struct SolConfig {
    /// Which chain this config represents (always `ChainId::Solana`).
    pub chain_id: ChainId,
    /// Native asset metadata (SOL, 9 decimals).
    pub native_asset: AssetInfo,
    /// BIP-44 derivation path (`m/44'/501'/0'/0'`).
    pub derivation_path: String,
    /// All networks (mainnet, devnet, testnet) for Solana.
    pub networks: Vec<BlockchainNetwork>,
    /// Solana JSON-RPC base URLs keyed by [`NetworkId`].
    pub api_urls: HashMap<NetworkId, Vec<Url>>,
    /// Block explorer URL templates keyed by [`NetworkId`].
    pub explorer_urls: HashMap<NetworkId, ExplorerUrls>,
}

/// Build the default [`SolConfig`] with mainnet + devnet + testnet.
///
/// Uses public Solana JSON-RPC endpoints and Solscan explorers.
#[must_use]
pub fn default_sol_config() -> SolConfig {
    let mainnet_id = NetworkId::new("solana-mainnet");
    let devnet_id = NetworkId::new("solana-devnet");
    let testnet_id = NetworkId::new("solana-testnet");

    let networks = vec![
        BlockchainNetwork {
            id: mainnet_id.clone(),
            label: "Solana Mainnet".into(),
            chain_id: ChainId::Solana,
            category: NetworkCategory::Mainnet,
            evm_chain_id: None,
        },
        BlockchainNetwork {
            id: devnet_id.clone(),
            label: "Solana Devnet".into(),
            chain_id: ChainId::Solana,
            category: NetworkCategory::Devnet,
            evm_chain_id: None,
        },
        BlockchainNetwork {
            id: testnet_id.clone(),
            label: "Solana Testnet".into(),
            chain_id: ChainId::Solana,
            category: NetworkCategory::Testnet,
            evm_chain_id: None,
        },
    ];

    let api_urls = build_api_urls(&mainnet_id, &devnet_id, &testnet_id);
    let explorer_urls = build_explorer_urls(&mainnet_id, &devnet_id, &testnet_id);

    SolConfig {
        chain_id: ChainId::Solana,
        native_asset: AssetInfo::sol(),
        derivation_path: dontyeet_crypto::derivation::paths::SOLANA.to_owned(),
        networks,
        api_urls,
        explorer_urls,
    }
}

/// Construct Solana JSON-RPC base URLs.
fn build_api_urls(
    mainnet: &NetworkId,
    devnet: &NetworkId,
    testnet: &NetworkId,
) -> HashMap<NetworkId, Vec<Url>> {
    let mut map = HashMap::new();
    map.insert(
        mainnet.clone(),
        parse_urls(&["https://api.mainnet-beta.solana.com"]),
    );
    map.insert(
        devnet.clone(),
        parse_urls(&["https://api.devnet.solana.com"]),
    );
    map.insert(
        testnet.clone(),
        parse_urls(&["https://api.testnet.solana.com"]),
    );
    map
}

/// Construct Solscan explorer URL templates.
fn build_explorer_urls(
    mainnet: &NetworkId,
    devnet: &NetworkId,
    testnet: &NetworkId,
) -> HashMap<NetworkId, ExplorerUrls> {
    let mut map = HashMap::new();
    map.insert(
        mainnet.clone(),
        ExplorerUrls::new(
            "https://solscan.io/account/{address}",
            "https://solscan.io/tx/{tx}",
        ),
    );
    map.insert(
        devnet.clone(),
        ExplorerUrls::new(
            "https://solscan.io/account/{address}?cluster=devnet",
            "https://solscan.io/tx/{tx}?cluster=devnet",
        ),
    );
    map.insert(
        testnet.clone(),
        ExplorerUrls::new(
            "https://solscan.io/account/{address}?cluster=testnet",
            "https://solscan.io/tx/{tx}?cluster=testnet",
        ),
    );
    map
}

/// Parse URL strings, silently dropping any that fail to parse.
fn parse_urls(raw: &[&str]) -> Vec<Url> {
    raw.iter().filter_map(|s| Url::parse(s).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_three_networks() {
        let cfg = default_sol_config();
        assert_eq!(cfg.networks.len(), 3);
    }

    #[test]
    fn mainnet_is_first_network() {
        let cfg = default_sol_config();
        assert_eq!(cfg.networks[0].id, NetworkId::new("solana-mainnet"));
        assert_eq!(cfg.networks[0].category, NetworkCategory::Mainnet);
    }

    #[test]
    fn api_urls_populated() {
        let cfg = default_sol_config();
        let mainnet = NetworkId::new("solana-mainnet");
        let urls = cfg.api_urls.get(&mainnet);
        assert!(urls.is_some());
        assert!(!urls.is_none_or(Vec::is_empty));
    }

    #[test]
    fn explorer_urls_populated() {
        let cfg = default_sol_config();
        let mainnet = NetworkId::new("solana-mainnet");
        let explorer = cfg.explorer_urls.get(&mainnet);
        assert!(explorer.is_some());
    }

    #[test]
    fn devnet_api_url_correct() {
        let cfg = default_sol_config();
        let devnet = NetworkId::new("solana-devnet");
        let urls = cfg.api_urls.get(&devnet).expect("devnet URLs");
        assert!(urls[0].as_str().contains("devnet"));
    }
}

// Rust guideline compliant 2026-05-02
