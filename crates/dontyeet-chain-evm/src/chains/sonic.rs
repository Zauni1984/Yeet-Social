//! Sonic mainnet + Blaze testnet factory.

use std::collections::HashMap;

use url::Url;

use dontyeet_primitives::asset::AssetInfo;
use dontyeet_primitives::chain::{ChainId, NetworkCategory, NetworkId};
use dontyeet_primitives::network::{BlockchainNetwork, ExplorerUrls};

use crate::config::EvmChainConfig;
use crate::plugin::EvmChainPlugin;

/// Create an [`EvmChainPlugin`] configured for Sonic (formerly Fantom).
///
/// Native token: S (not FTM).
/// Networks: Mainnet (chain ID 146), Blaze testnet (chain ID 57054).
#[must_use]
pub fn sonic_plugin() -> EvmChainPlugin {
    let mainnet_id = NetworkId::new("sonic-mainnet");
    let blaze_id = NetworkId::new("sonic-blaze");

    let networks = vec![
        BlockchainNetwork {
            id: mainnet_id.clone(),
            label: "Sonic Mainnet".into(),
            chain_id: ChainId::Sonic,
            category: NetworkCategory::Mainnet,
            evm_chain_id: Some(146),
        },
        BlockchainNetwork {
            id: blaze_id.clone(),
            label: "Sonic Blaze".into(),
            chain_id: ChainId::Sonic,
            category: NetworkCategory::Testnet,
            evm_chain_id: Some(57_054),
        },
    ];

    let mut rpc_urls = HashMap::new();
    rpc_urls.insert(
        mainnet_id.clone(),
        vec![parse_url("https://rpc.soniclabs.com")],
    );
    rpc_urls.insert(
        blaze_id.clone(),
        vec![parse_url("https://rpc.blaze.soniclabs.com")],
    );

    let mut explorer_urls = HashMap::new();
    explorer_urls.insert(
        mainnet_id,
        ExplorerUrls::new(
            "https://sonicscan.org/address/{address}",
            "https://sonicscan.org/tx/{tx}",
        ),
    );
    explorer_urls.insert(
        blaze_id,
        ExplorerUrls::new(
            "https://testnet.sonicscan.org/address/{address}",
            "https://testnet.sonicscan.org/tx/{tx}",
        ),
    );

    // Sonic has no Etherscan-compatible API.
    let config = EvmChainConfig {
        chain_id: ChainId::Sonic,
        native_asset: AssetInfo::sonic(),
        derivation_path: dontyeet_crypto::paths::SONIC.into(),
        evm_chain_id_mainnet: 146,
        networks,
        rpc_urls,
        explorer_urls,
        explorer_api_urls: HashMap::new(),
        explorer_api_key: None,
    };

    EvmChainPlugin::new(config)
}

/// Parse a URL string that is known to be valid at compile time.
fn parse_url(s: &str) -> Url {
    s.parse().expect("hardcoded URL must be valid")
}

// Rust guideline compliant 2026-05-02
