//! Algorand chain configuration.
//!
//! Parameterizes the Algorand plugin with mainnet and testnet network
//! metadata, API URLs, and explorer templates.

use std::collections::HashMap;

use url::Url;

use dontyeet_primitives::asset::AssetInfo;
use dontyeet_primitives::chain::{ChainId, NetworkCategory, NetworkId};
use dontyeet_primitives::network::{BlockchainNetwork, ExplorerUrls};

/// Complete configuration for the Algorand chain plugin.
#[derive(Debug, Clone)]
pub struct AlgoConfig {
    /// Which chain this config represents (always `ChainId::Algorand`).
    pub chain_id: ChainId,
    /// Native asset metadata (ALGO, 6 decimals).
    pub native_asset: AssetInfo,
    /// BIP-44 derivation path.
    pub derivation_path: String,
    /// All networks for Algorand.
    pub networks: Vec<BlockchainNetwork>,
    /// Algod REST API base URLs keyed by [`NetworkId`].
    pub api_urls: HashMap<NetworkId, Vec<Url>>,
    /// Block explorer URL templates keyed by [`NetworkId`].
    pub explorer_urls: HashMap<NetworkId, ExplorerUrls>,
}

/// Build the default [`AlgoConfig`] with mainnet + testnet.
///
/// Uses Nodely (formerly `AlgoNode`) public Algod endpoints.
#[must_use]
pub fn default_algo_config() -> AlgoConfig {
    let mainnet_id = NetworkId::new("algorand-mainnet");
    let testnet_id = NetworkId::new("algorand-testnet");

    let networks = vec![
        BlockchainNetwork {
            id: mainnet_id.clone(),
            label: "Algorand Mainnet".into(),
            chain_id: ChainId::Algorand,
            category: NetworkCategory::Mainnet,
            evm_chain_id: None,
        },
        BlockchainNetwork {
            id: testnet_id.clone(),
            label: "Algorand Testnet".into(),
            chain_id: ChainId::Algorand,
            category: NetworkCategory::Testnet,
            evm_chain_id: None,
        },
    ];

    let api_urls = build_api_urls(&mainnet_id, &testnet_id);
    let explorer_urls = build_explorer_urls(&mainnet_id, &testnet_id);

    AlgoConfig {
        chain_id: ChainId::Algorand,
        native_asset: AssetInfo::algo(),
        derivation_path: dontyeet_crypto::derivation::paths::ALGORAND.to_owned(),
        networks,
        api_urls,
        explorer_urls,
    }
}

/// Construct Nodely Algod REST API base URLs.
fn build_api_urls(mainnet: &NetworkId, testnet: &NetworkId) -> HashMap<NetworkId, Vec<Url>> {
    let mut map = HashMap::new();
    map.insert(
        mainnet.clone(),
        parse_urls(&["https://mainnet-api.4160.nodely.dev"]),
    );
    map.insert(
        testnet.clone(),
        parse_urls(&["https://testnet-api.4160.nodely.dev"]),
    );
    map
}

/// Construct explorer URL templates.
fn build_explorer_urls(
    mainnet: &NetworkId,
    testnet: &NetworkId,
) -> HashMap<NetworkId, ExplorerUrls> {
    let mut map = HashMap::new();
    map.insert(
        mainnet.clone(),
        ExplorerUrls::new(
            "https://allo.info/account/{address}",
            "https://allo.info/tx/{tx}",
        ),
    );
    map.insert(
        testnet.clone(),
        ExplorerUrls::new(
            "https://testnet.allo.info/account/{address}",
            "https://testnet.allo.info/tx/{tx}",
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
    fn default_config_has_two_networks() {
        let cfg = default_algo_config();
        assert_eq!(cfg.networks.len(), 2);
    }

    #[test]
    fn mainnet_is_first_network() {
        let cfg = default_algo_config();
        assert_eq!(cfg.networks[0].id, NetworkId::new("algorand-mainnet"));
        assert_eq!(cfg.networks[0].category, NetworkCategory::Mainnet);
    }

    #[test]
    fn api_urls_populated() {
        let cfg = default_algo_config();
        let mainnet = NetworkId::new("algorand-mainnet");
        let urls = cfg.api_urls.get(&mainnet);
        assert!(urls.is_some());
        assert!(!urls.is_none_or(Vec::is_empty));
    }
}

// Rust guideline compliant 2026-05-02
