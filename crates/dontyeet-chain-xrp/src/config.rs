//! XRP Ledger chain configuration.
//!
//! Parameterizes the XRP plugin with mainnet and testnet network metadata,
//! JSON-RPC API URLs, and explorer templates.

use std::collections::HashMap;

use url::Url;

use dontyeet_primitives::asset::AssetInfo;
use dontyeet_primitives::chain::{ChainId, NetworkCategory, NetworkId};
use dontyeet_primitives::network::{BlockchainNetwork, ExplorerUrls};

/// Complete configuration for the XRP Ledger chain plugin.
///
/// Covers mainnet and testnet networks. API endpoints point to the
/// XRP Ledger JSON-RPC servers.
#[derive(Debug, Clone)]
pub struct XrpConfig {
    /// Which chain this config represents (always `ChainId::Xrp`).
    pub chain_id: ChainId,
    /// Native asset metadata (XRP, 6 decimals).
    pub native_asset: AssetInfo,
    /// BIP-44 derivation path (`m/44'/144'/0'/0/0`).
    pub derivation_path: String,
    /// All networks (mainnet, testnet) for XRP.
    pub networks: Vec<BlockchainNetwork>,
    /// JSON-RPC API base URLs keyed by [`NetworkId`].
    pub api_urls: HashMap<NetworkId, Vec<Url>>,
    /// Block explorer URL templates keyed by [`NetworkId`].
    pub explorer_urls: HashMap<NetworkId, ExplorerUrls>,
}

/// Build the default [`XrpConfig`] with mainnet + testnet.
///
/// Uses XRP Ledger JSON-RPC endpoints and `XRPScan` explorers.
#[must_use]
pub fn default_xrp_config() -> XrpConfig {
    let mainnet_id = NetworkId::new("xrp-mainnet");
    let testnet_id = NetworkId::new("xrp-testnet");

    let networks = vec![
        BlockchainNetwork {
            id: mainnet_id.clone(),
            label: "XRP Mainnet".into(),
            chain_id: ChainId::Xrp,
            category: NetworkCategory::Mainnet,
            evm_chain_id: None,
        },
        BlockchainNetwork {
            id: testnet_id.clone(),
            label: "XRP Testnet".into(),
            chain_id: ChainId::Xrp,
            category: NetworkCategory::Testnet,
            evm_chain_id: None,
        },
    ];

    let api_urls = build_api_urls(&mainnet_id, &testnet_id);
    let explorer_urls = build_explorer_urls(&mainnet_id, &testnet_id);

    XrpConfig {
        chain_id: ChainId::Xrp,
        native_asset: AssetInfo::xrp(),
        derivation_path: dontyeet_crypto::derivation::paths::XRP.to_owned(),
        networks,
        api_urls,
        explorer_urls,
    }
}

/// Construct XRP Ledger JSON-RPC API base URLs.
fn build_api_urls(mainnet: &NetworkId, testnet: &NetworkId) -> HashMap<NetworkId, Vec<Url>> {
    let mut map = HashMap::new();
    map.insert(
        mainnet.clone(),
        parse_urls(&["https://s1.ripple.com:51234/"]),
    );
    map.insert(
        testnet.clone(),
        parse_urls(&["https://s.altnet.rippletest.net:51234/"]),
    );
    map
}

/// Construct `XRPScan` explorer URL templates.
fn build_explorer_urls(
    mainnet: &NetworkId,
    testnet: &NetworkId,
) -> HashMap<NetworkId, ExplorerUrls> {
    let mut map = HashMap::new();
    map.insert(
        mainnet.clone(),
        ExplorerUrls::new(
            "https://xrpscan.com/account/{address}",
            "https://xrpscan.com/tx/{tx}",
        ),
    );
    map.insert(
        testnet.clone(),
        ExplorerUrls::new(
            "https://testnet.xrpscan.com/account/{address}",
            "https://testnet.xrpscan.com/tx/{tx}",
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
        let cfg = default_xrp_config();
        assert_eq!(cfg.networks.len(), 2);
    }

    #[test]
    fn mainnet_is_first_network() {
        let cfg = default_xrp_config();
        assert_eq!(cfg.networks[0].id, NetworkId::new("xrp-mainnet"));
        assert_eq!(cfg.networks[0].category, NetworkCategory::Mainnet);
    }

    #[test]
    fn api_urls_populated() {
        let cfg = default_xrp_config();
        let mainnet = NetworkId::new("xrp-mainnet");
        let urls = cfg.api_urls.get(&mainnet);
        assert!(urls.is_some());
        assert!(!urls.is_none_or(Vec::is_empty));
    }

    #[test]
    fn testnet_api_urls_populated() {
        let cfg = default_xrp_config();
        let testnet = NetworkId::new("xrp-testnet");
        let urls = cfg.api_urls.get(&testnet);
        assert!(urls.is_some());
        assert!(!urls.is_none_or(Vec::is_empty));
    }

    #[test]
    fn explorer_urls_populated() {
        let cfg = default_xrp_config();
        let mainnet = NetworkId::new("xrp-mainnet");
        let explorer = cfg.explorer_urls.get(&mainnet);
        assert!(explorer.is_some());
    }
}

// Rust guideline compliant 2026-05-02
