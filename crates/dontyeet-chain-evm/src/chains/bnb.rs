//! BNB Smart Chain mainnet + testnet factory.

use std::collections::HashMap;

use url::Url;

use dontyeet_primitives::asset::AssetInfo;
use dontyeet_primitives::chain::{ChainId, NetworkCategory, NetworkId};
use dontyeet_primitives::network::{BlockchainNetwork, ExplorerUrls};

use crate::config::EvmChainConfig;
use crate::plugin::EvmChainPlugin;

/// Create an [`EvmChainPlugin`] configured for BNB Smart Chain.
///
/// Networks: Mainnet (chain ID 56), Testnet (chain ID 97).
#[must_use]
pub fn bnb_plugin() -> EvmChainPlugin {
    let mainnet_id = NetworkId::new("bnb-mainnet");
    let testnet_id = NetworkId::new("bnb-testnet");

    let networks = vec![
        BlockchainNetwork {
            id: mainnet_id.clone(),
            label: "BNB Smart Chain".into(),
            chain_id: ChainId::Bnb,
            category: NetworkCategory::Mainnet,
            evm_chain_id: Some(56),
        },
        BlockchainNetwork {
            id: testnet_id.clone(),
            label: "BNB Testnet".into(),
            chain_id: ChainId::Bnb,
            category: NetworkCategory::Testnet,
            evm_chain_id: Some(97),
        },
    ];

    let mut rpc_urls = HashMap::new();
    rpc_urls.insert(
        mainnet_id.clone(),
        vec![parse_url("https://bsc-dataseed1.bnbchain.org")],
    );
    rpc_urls.insert(
        testnet_id.clone(),
        vec![parse_url("https://data-seed-prebsc-1-s1.bnbchain.org:8545")],
    );

    let mut explorer_urls = HashMap::new();
    explorer_urls.insert(
        mainnet_id,
        ExplorerUrls::new(
            "https://bscscan.com/address/{address}",
            "https://bscscan.com/tx/{tx}",
        ),
    );
    explorer_urls.insert(
        testnet_id,
        ExplorerUrls::new(
            "https://testnet.bscscan.com/address/{address}",
            "https://testnet.bscscan.com/tx/{tx}",
        ),
    );

    let mut explorer_api_urls = HashMap::new();
    explorer_api_urls.insert(
        NetworkId::new("bnb-mainnet"),
        parse_url("https://api.bscscan.com/api"),
    );

    let config = EvmChainConfig {
        chain_id: ChainId::Bnb,
        native_asset: AssetInfo::bnb(),
        derivation_path: dontyeet_crypto::paths::BNB.into(),
        evm_chain_id_mainnet: 56,
        networks,
        rpc_urls,
        explorer_urls,
        explorer_api_urls,
        explorer_api_key: None,
    };

    EvmChainPlugin::new(config)
}

/// Parse a URL string that is known to be valid at compile time.
fn parse_url(s: &str) -> Url {
    s.parse().expect("hardcoded URL must be valid")
}

// Rust guideline compliant 2026-05-02
