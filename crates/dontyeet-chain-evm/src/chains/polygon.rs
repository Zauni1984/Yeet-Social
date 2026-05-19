//! Polygon `PoS` mainnet + Amoy testnet factory.

use std::collections::HashMap;

use url::Url;

use dontyeet_primitives::asset::AssetInfo;
use dontyeet_primitives::chain::{ChainId, NetworkCategory, NetworkId};
use dontyeet_primitives::network::{BlockchainNetwork, ExplorerUrls};

use crate::config::EvmChainConfig;
use crate::plugin::EvmChainPlugin;

/// Create an [`EvmChainPlugin`] configured for Polygon `PoS`.
///
/// Native token: POL (formerly MATIC).
/// Networks: Mainnet (chain ID 137), Amoy testnet (chain ID 80002).
#[must_use]
pub fn polygon_plugin() -> EvmChainPlugin {
    let mainnet_id = NetworkId::new("polygon-mainnet");
    let amoy_id = NetworkId::new("polygon-amoy");

    let networks = vec![
        BlockchainNetwork {
            id: mainnet_id.clone(),
            label: "Polygon Mainnet".into(),
            chain_id: ChainId::Polygon,
            category: NetworkCategory::Mainnet,
            evm_chain_id: Some(137),
        },
        BlockchainNetwork {
            id: amoy_id.clone(),
            label: "Polygon Amoy".into(),
            chain_id: ChainId::Polygon,
            category: NetworkCategory::Testnet,
            evm_chain_id: Some(80_002),
        },
    ];

    let mut rpc_urls = HashMap::new();
    rpc_urls.insert(
        mainnet_id.clone(),
        vec![
            parse_url("https://polygon-rpc.com"),
            parse_url("https://polygon-mainnet.public.blastapi.io"),
        ],
    );
    rpc_urls.insert(
        amoy_id.clone(),
        vec![parse_url("https://rpc-amoy.polygon.technology")],
    );

    let mut explorer_urls = HashMap::new();
    explorer_urls.insert(
        mainnet_id,
        ExplorerUrls::new(
            "https://polygonscan.com/address/{address}",
            "https://polygonscan.com/tx/{tx}",
        ),
    );
    explorer_urls.insert(
        amoy_id,
        ExplorerUrls::new(
            "https://amoy.polygonscan.com/address/{address}",
            "https://amoy.polygonscan.com/tx/{tx}",
        ),
    );

    let mut explorer_api_urls = HashMap::new();
    explorer_api_urls.insert(
        NetworkId::new("polygon-mainnet"),
        parse_url("https://api.polygonscan.com/api"),
    );

    let config = EvmChainConfig {
        chain_id: ChainId::Polygon,
        native_asset: AssetInfo::pol(),
        derivation_path: dontyeet_crypto::paths::POLYGON.into(),
        evm_chain_id_mainnet: 137,
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
