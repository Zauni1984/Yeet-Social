//! Ethereum mainnet + Sepolia testnet factory.

use std::collections::HashMap;

use url::Url;

use dontyeet_primitives::asset::AssetInfo;
use dontyeet_primitives::chain::{ChainId, NetworkCategory, NetworkId};
use dontyeet_primitives::network::{BlockchainNetwork, ExplorerUrls};

use crate::config::EvmChainConfig;
use crate::plugin::EvmChainPlugin;

/// Create an [`EvmChainPlugin`] configured for Ethereum.
///
/// Networks: Mainnet (chain ID 1), Sepolia testnet (chain ID 11155111).
#[must_use]
pub fn ethereum_plugin() -> EvmChainPlugin {
    let mainnet_id = NetworkId::new("ethereum-mainnet");
    let sepolia_id = NetworkId::new("ethereum-sepolia");

    let networks = vec![
        BlockchainNetwork {
            id: mainnet_id.clone(),
            label: "Ethereum Mainnet".into(),
            chain_id: ChainId::Ethereum,
            category: NetworkCategory::Mainnet,
            evm_chain_id: Some(1),
        },
        BlockchainNetwork {
            id: sepolia_id.clone(),
            label: "Ethereum Sepolia".into(),
            chain_id: ChainId::Ethereum,
            category: NetworkCategory::Testnet,
            evm_chain_id: Some(11_155_111),
        },
    ];

    let mut rpc_urls = HashMap::new();
    rpc_urls.insert(
        mainnet_id.clone(),
        vec![
            parse_url("https://eth.llamarpc.com"),
            parse_url("https://ethereum-rpc.publicnode.com"),
        ],
    );
    rpc_urls.insert(
        sepolia_id.clone(),
        vec![parse_url("https://ethereum-sepolia-rpc.publicnode.com")],
    );

    let mut explorer_urls = HashMap::new();
    explorer_urls.insert(
        mainnet_id,
        ExplorerUrls::new(
            "https://etherscan.io/address/{address}",
            "https://etherscan.io/tx/{tx}",
        ),
    );
    explorer_urls.insert(
        sepolia_id,
        ExplorerUrls::new(
            "https://sepolia.etherscan.io/address/{address}",
            "https://sepolia.etherscan.io/tx/{tx}",
        ),
    );

    let mut explorer_api_urls = HashMap::new();
    explorer_api_urls.insert(
        NetworkId::new("ethereum-mainnet"),
        parse_url("https://api.etherscan.io/api"),
    );
    explorer_api_urls.insert(
        NetworkId::new("ethereum-sepolia"),
        parse_url("https://api-sepolia.etherscan.io/api"),
    );

    let config = EvmChainConfig {
        chain_id: ChainId::Ethereum,
        native_asset: AssetInfo::eth(),
        derivation_path: dontyeet_crypto::paths::ETHEREUM.into(),
        evm_chain_id_mainnet: 1,
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
