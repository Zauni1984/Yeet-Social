//! TRON chain configuration.
//!
//! Parameterizes the TRON plugin with mainnet, Shasta testnet, and Nile
//! testnet network metadata, API URLs, and explorer templates.

use std::collections::HashMap;

use url::Url;

use dontyeet_primitives::asset::AssetInfo;
use dontyeet_primitives::chain::{ChainId, NetworkCategory, NetworkId};
use dontyeet_primitives::network::{BlockchainNetwork, ExplorerUrls};

/// Complete configuration for the TRON chain plugin.
///
/// Covers mainnet, Shasta testnet, and Nile testnet. API endpoints
/// point to `TronGrid` REST API.
#[derive(Debug, Clone)]
pub struct TronConfig {
    /// Which chain this config represents (always `ChainId::Tron`).
    pub chain_id: ChainId,
    /// Native asset metadata (TRX, 6 decimals).
    pub native_asset: AssetInfo,
    /// BIP-44 derivation path (`m/44'/195'/0'/0/0`).
    pub derivation_path: String,
    /// All networks (mainnet, Shasta, Nile) for TRON.
    pub networks: Vec<BlockchainNetwork>,
    /// `TronGrid` REST API base URLs keyed by [`NetworkId`].
    pub api_urls: HashMap<NetworkId, Vec<Url>>,
    /// Block explorer URL templates keyed by [`NetworkId`].
    pub explorer_urls: HashMap<NetworkId, ExplorerUrls>,
}

/// Build the default [`TronConfig`] with mainnet + Shasta + Nile.
///
/// Uses `TronGrid` REST endpoints and `TronScan` explorers.
#[must_use]
pub fn default_tron_config() -> TronConfig {
    let mainnet_id = NetworkId::new("tron-mainnet");
    let shasta_id = NetworkId::new("tron-shasta");
    let nile_id = NetworkId::new("tron-nile");

    let networks = vec![
        BlockchainNetwork {
            id: mainnet_id.clone(),
            label: "TRON Mainnet".into(),
            chain_id: ChainId::Tron,
            category: NetworkCategory::Mainnet,
            evm_chain_id: None,
        },
        BlockchainNetwork {
            id: shasta_id.clone(),
            label: "TRON Shasta Testnet".into(),
            chain_id: ChainId::Tron,
            category: NetworkCategory::Testnet,
            evm_chain_id: None,
        },
        BlockchainNetwork {
            id: nile_id.clone(),
            label: "TRON Nile Testnet".into(),
            chain_id: ChainId::Tron,
            category: NetworkCategory::Testnet,
            evm_chain_id: None,
        },
    ];

    let api_urls = build_api_urls(&mainnet_id, &shasta_id, &nile_id);
    let explorer_urls = build_explorer_urls(&mainnet_id, &shasta_id, &nile_id);

    TronConfig {
        chain_id: ChainId::Tron,
        native_asset: AssetInfo::trx(),
        derivation_path: dontyeet_crypto::derivation::paths::TRON.to_owned(),
        networks,
        api_urls,
        explorer_urls,
    }
}

/// Construct `TronGrid` REST API base URLs.
fn build_api_urls(
    mainnet: &NetworkId,
    shasta: &NetworkId,
    nile: &NetworkId,
) -> HashMap<NetworkId, Vec<Url>> {
    let mut map = HashMap::new();
    map.insert(mainnet.clone(), parse_urls(&["https://api.trongrid.io"]));
    map.insert(
        shasta.clone(),
        parse_urls(&["https://api.shasta.trongrid.io"]),
    );
    map.insert(nile.clone(), parse_urls(&["https://nile.trongrid.io"]));
    map
}

/// Construct `TronScan` explorer URL templates.
fn build_explorer_urls(
    mainnet: &NetworkId,
    shasta: &NetworkId,
    nile: &NetworkId,
) -> HashMap<NetworkId, ExplorerUrls> {
    let mut map = HashMap::new();
    map.insert(
        mainnet.clone(),
        ExplorerUrls::new(
            "https://tronscan.org/#/address/{address}",
            "https://tronscan.org/#/transaction/{tx}",
        ),
    );
    map.insert(
        shasta.clone(),
        ExplorerUrls::new(
            "https://shasta.tronscan.org/#/address/{address}",
            "https://shasta.tronscan.org/#/transaction/{tx}",
        ),
    );
    map.insert(
        nile.clone(),
        ExplorerUrls::new(
            "https://nile.tronscan.org/#/address/{address}",
            "https://nile.tronscan.org/#/transaction/{tx}",
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
        let cfg = default_tron_config();
        assert_eq!(cfg.networks.len(), 3);
    }

    #[test]
    fn mainnet_is_first_network() {
        let cfg = default_tron_config();
        assert_eq!(cfg.networks[0].id, NetworkId::new("tron-mainnet"));
        assert_eq!(cfg.networks[0].category, NetworkCategory::Mainnet);
    }

    #[test]
    fn api_urls_populated() {
        let cfg = default_tron_config();
        let mainnet = NetworkId::new("tron-mainnet");
        let urls = cfg.api_urls.get(&mainnet);
        assert!(urls.is_some());
        assert!(!urls.is_none_or(Vec::is_empty));
    }

    #[test]
    fn explorer_urls_populated() {
        let cfg = default_tron_config();
        let mainnet = NetworkId::new("tron-mainnet");
        let explorer = cfg.explorer_urls.get(&mainnet);
        assert!(explorer.is_some());
    }

    #[test]
    fn shasta_api_url_correct() {
        let cfg = default_tron_config();
        let shasta = NetworkId::new("tron-shasta");
        let urls = cfg.api_urls.get(&shasta).expect("shasta urls");
        assert!(urls[0].as_str().contains("shasta"));
    }

    #[test]
    fn nile_api_url_correct() {
        let cfg = default_tron_config();
        let nile = NetworkId::new("tron-nile");
        let urls = cfg.api_urls.get(&nile).expect("nile urls");
        assert!(urls[0].as_str().contains("nile"));
    }
}

// Rust guideline compliant 2026-05-02
