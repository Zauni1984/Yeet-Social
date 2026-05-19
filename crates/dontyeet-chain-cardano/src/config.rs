//! Cardano chain configuration.
//!
//! Parameterizes the Cardano plugin with mainnet, preprod, and preview
//! network metadata, Blockfrost API URLs, and explorer templates.

use std::collections::HashMap;

use url::Url;

use dontyeet_primitives::asset::AssetInfo;
use dontyeet_primitives::chain::{ChainId, NetworkCategory, NetworkId};
use dontyeet_primitives::network::{BlockchainNetwork, ExplorerUrls};

/// Complete configuration for the Cardano chain plugin.
#[derive(Debug, Clone)]
pub struct CardanoConfig {
    /// Which chain this config represents (always `ChainId::Cardano`).
    pub chain_id: ChainId,
    /// Native asset metadata (ADA, 6 decimals).
    pub native_asset: AssetInfo,
    /// CIP-1852 derivation path.
    pub derivation_path: String,
    /// All networks for Cardano.
    pub networks: Vec<BlockchainNetwork>,
    /// Blockfrost REST API base URLs keyed by [`NetworkId`].
    pub api_urls: HashMap<NetworkId, Vec<Url>>,
    /// Block explorer URL templates keyed by [`NetworkId`].
    pub explorer_urls: HashMap<NetworkId, ExplorerUrls>,
    /// Blockfrost `project_id` API key sent on every request.
    ///
    /// Without it, every Blockfrost call returns 403 — Cardano features
    /// (balance, history, fee estimation, broadcast, RPC health) are
    /// effectively non-functional. Read from `[cardano] blockfrost_project_id`
    /// in `dontyeet.toml` or the `BLOCKFROST_PROJECT_ID` env var.
    pub blockfrost_project_id: Option<String>,
}

/// Build the default [`CardanoConfig`] with mainnet + preprod + preview.
///
/// Uses Blockfrost REST v0 endpoints (requires `project_id` header).
#[must_use]
pub fn default_cardano_config() -> CardanoConfig {
    let mainnet_id = NetworkId::new("cardano-mainnet");
    let preprod_id = NetworkId::new("cardano-preprod");
    let preview_id = NetworkId::new("cardano-preview");

    let networks = vec![
        BlockchainNetwork {
            id: mainnet_id.clone(),
            label: "Cardano Mainnet".into(),
            chain_id: ChainId::Cardano,
            category: NetworkCategory::Mainnet,
            evm_chain_id: None,
        },
        BlockchainNetwork {
            id: preprod_id.clone(),
            label: "Cardano Preprod".into(),
            chain_id: ChainId::Cardano,
            category: NetworkCategory::Testnet,
            evm_chain_id: None,
        },
        BlockchainNetwork {
            id: preview_id.clone(),
            label: "Cardano Preview".into(),
            chain_id: ChainId::Cardano,
            category: NetworkCategory::Testnet,
            evm_chain_id: None,
        },
    ];

    let api_urls = build_api_urls(&mainnet_id, &preprod_id, &preview_id);
    let explorer_urls = build_explorer_urls(&mainnet_id, &preprod_id, &preview_id);

    CardanoConfig {
        chain_id: ChainId::Cardano,
        native_asset: AssetInfo::ada(),
        derivation_path: dontyeet_crypto::derivation::paths::CARDANO.to_owned(),
        networks,
        api_urls,
        explorer_urls,
        blockfrost_project_id: None,
    }
}

/// Construct Blockfrost REST API v0 base URLs.
fn build_api_urls(
    mainnet: &NetworkId,
    preprod: &NetworkId,
    preview: &NetworkId,
) -> HashMap<NetworkId, Vec<Url>> {
    let mut map = HashMap::new();
    map.insert(
        mainnet.clone(),
        parse_urls(&["https://cardano-mainnet.blockfrost.io/api/v0"]),
    );
    map.insert(
        preprod.clone(),
        parse_urls(&["https://cardano-preprod.blockfrost.io/api/v0"]),
    );
    map.insert(
        preview.clone(),
        parse_urls(&["https://cardano-preview.blockfrost.io/api/v0"]),
    );
    map
}

/// Construct `CardanoScan` explorer URL templates.
fn build_explorer_urls(
    mainnet: &NetworkId,
    preprod: &NetworkId,
    preview: &NetworkId,
) -> HashMap<NetworkId, ExplorerUrls> {
    let mut map = HashMap::new();
    map.insert(
        mainnet.clone(),
        ExplorerUrls::new(
            "https://cardanoscan.io/address/{address}",
            "https://cardanoscan.io/transaction/{tx}",
        ),
    );
    map.insert(
        preprod.clone(),
        ExplorerUrls::new(
            "https://preprod.cardanoscan.io/address/{address}",
            "https://preprod.cardanoscan.io/transaction/{tx}",
        ),
    );
    map.insert(
        preview.clone(),
        ExplorerUrls::new(
            "https://preview.cardanoscan.io/address/{address}",
            "https://preview.cardanoscan.io/transaction/{tx}",
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
        let cfg = default_cardano_config();
        assert_eq!(cfg.networks.len(), 3);
    }

    #[test]
    fn mainnet_is_first_network() {
        let cfg = default_cardano_config();
        assert_eq!(cfg.networks[0].id, NetworkId::new("cardano-mainnet"));
        assert_eq!(cfg.networks[0].category, NetworkCategory::Mainnet);
    }

    #[test]
    fn api_urls_populated() {
        let cfg = default_cardano_config();
        let mainnet = NetworkId::new("cardano-mainnet");
        let urls = cfg.api_urls.get(&mainnet);
        assert!(urls.is_some());
        assert!(!urls.is_none_or(Vec::is_empty));
    }
}

// Rust guideline compliant 2026-05-02
