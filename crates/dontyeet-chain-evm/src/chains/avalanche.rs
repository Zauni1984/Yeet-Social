//! Avalanche C-Chain mainnet + Fuji testnet factory.

use std::collections::HashMap;

use url::Url;

use dontyeet_primitives::asset::AssetInfo;
use dontyeet_primitives::chain::{ChainId, NetworkCategory, NetworkId};
use dontyeet_primitives::network::{BlockchainNetwork, ExplorerUrls};

use crate::config::EvmChainConfig;
use crate::plugin::EvmChainPlugin;

/// Create an [`EvmChainPlugin`] configured for Avalanche C-Chain.
///
/// Networks: Mainnet (chain ID 43114), Fuji testnet (chain ID 43113).
#[must_use]
pub fn avalanche_plugin() -> EvmChainPlugin {
    let mainnet_id = NetworkId::new("avalanche-mainnet");
    let fuji_id = NetworkId::new("avalanche-fuji");

    let networks = vec![
        BlockchainNetwork {
            id: mainnet_id.clone(),
            label: "Avalanche C-Chain".into(),
            chain_id: ChainId::Avalanche,
            category: NetworkCategory::Mainnet,
            evm_chain_id: Some(43_114),
        },
        BlockchainNetwork {
            id: fuji_id.clone(),
            label: "Avalanche Fuji".into(),
            chain_id: ChainId::Avalanche,
            category: NetworkCategory::Testnet,
            evm_chain_id: Some(43_113),
        },
    ];

    let mut rpc_urls = HashMap::new();
    rpc_urls.insert(
        mainnet_id.clone(),
        vec![parse_url("https://api.avax.network/ext/bc/C/rpc")],
    );
    rpc_urls.insert(
        fuji_id.clone(),
        vec![parse_url("https://api.avax-test.network/ext/bc/C/rpc")],
    );

    let mut explorer_urls = HashMap::new();
    explorer_urls.insert(
        mainnet_id,
        ExplorerUrls::new(
            "https://subnets.avax.network/c-chain/address/{address}",
            "https://subnets.avax.network/c-chain/tx/{tx}",
        ),
    );
    explorer_urls.insert(
        fuji_id,
        ExplorerUrls::new(
            "https://subnets-test.avax.network/c-chain/address/{address}",
            "https://subnets-test.avax.network/c-chain/tx/{tx}",
        ),
    );

    let mut explorer_api_urls = HashMap::new();
    explorer_api_urls.insert(
        NetworkId::new("avalanche-mainnet"),
        parse_url("https://api.snowtrace.io/api"),
    );

    let config = EvmChainConfig {
        chain_id: ChainId::Avalanche,
        native_asset: AssetInfo::avax(),
        derivation_path: dontyeet_crypto::paths::AVALANCHE.into(),
        evm_chain_id_mainnet: 43_114,
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
