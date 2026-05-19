//! Bitcoin chain configuration.
//!
//! Parameterizes the Bitcoin plugin with mainnet, testnet4, and signet
//! network metadata, API URLs, and explorer templates.

use std::collections::HashMap;

use url::Url;

use dontyeet_primitives::asset::AssetInfo;
use dontyeet_primitives::chain::{ChainId, NetworkCategory, NetworkId};
use dontyeet_primitives::network::{BlockchainNetwork, ExplorerUrls};

/// Per-network address-format parameters.
///
/// All segwit-compatible UTXO chains share the same script grammar; what
/// differs between them is the bech32 HRP and the legacy `Base58Check`
/// version bytes. Carrying these in config lets the same plugin shape
/// serve Bitcoin, Litecoin, Bitcoin testnet variants, and any future
/// segwit-compatible chain without code changes.
#[derive(Debug, Clone)]
pub struct AddressFormat {
    /// Bech32 HRP for native segwit (P2WPKH) addresses, e.g. `"bc"`,
    /// `"tb"`, `"ltc"`, `"tltc"`.
    pub bech32_hrp: String,
    /// `Base58Check` version byte for legacy P2PKH addresses on this
    /// network (e.g. `0x00` Bitcoin mainnet, `0x6F` Bitcoin testnet,
    /// `0x30` Litecoin mainnet).
    pub base58_p2pkh_version: u8,
    /// `Base58Check` version byte for legacy P2SH addresses on this
    /// network (e.g. `0x05` Bitcoin mainnet, `0xC4` Bitcoin testnet,
    /// `0x32` Litecoin mainnet).
    pub base58_p2sh_version: u8,
}

impl AddressFormat {
    /// Bitcoin mainnet — `bc1...` segwit, `1...` / `3...` legacy.
    #[must_use]
    pub fn bitcoin_mainnet() -> Self {
        Self {
            bech32_hrp: "bc".into(),
            base58_p2pkh_version: 0x00,
            base58_p2sh_version: 0x05,
        }
    }

    /// Bitcoin testnet/signet — `tb1...` segwit, `m`/`n`/`2...` legacy.
    #[must_use]
    pub fn bitcoin_testnet() -> Self {
        Self {
            bech32_hrp: "tb".into(),
            base58_p2pkh_version: 0x6F,
            base58_p2sh_version: 0xC4,
        }
    }
}

/// Complete configuration for the Bitcoin chain plugin.
///
/// Covers mainnet, testnet4, and signet networks. API endpoints point to
/// Mempool.space REST API.
#[derive(Debug, Clone)]
pub struct BtcConfig {
    /// Which chain this config represents (always `ChainId::Bitcoin` for
    /// the built-in default; `ChainId::Other(...)` for custom UTXO chains
    /// constructed via the admin API).
    pub chain_id: ChainId,
    /// Native asset metadata (BTC, 8 decimals).
    pub native_asset: AssetInfo,
    /// BIP-84 derivation path for segwit (`m/84'/0'/0'/0/0`).
    pub derivation_path: String,
    /// All networks (mainnet, testnet4, signet) for Bitcoin.
    pub networks: Vec<BlockchainNetwork>,
    /// Mempool.space REST API base URLs keyed by [`NetworkId`].
    pub api_urls: HashMap<NetworkId, Vec<Url>>,
    /// Block explorer URL templates keyed by [`NetworkId`].
    pub explorer_urls: HashMap<NetworkId, ExplorerUrls>,
    /// Per-network address-format parameters (bech32 HRP, version bytes).
    /// Required: every entry in [`Self::networks`] must have a matching
    /// entry here, or address derivation fails for that network.
    pub address_formats: HashMap<NetworkId, AddressFormat>,
}

/// Build the default [`BtcConfig`] with mainnet + testnet4 + signet.
///
/// Uses Mempool.space REST endpoints and explorers.
#[must_use]
pub fn default_btc_config() -> BtcConfig {
    let mainnet_id = NetworkId::new("bitcoin-mainnet");
    let testnet4_id = NetworkId::new("bitcoin-testnet4");
    let signet_id = NetworkId::new("bitcoin-signet");

    let networks = vec![
        BlockchainNetwork {
            id: mainnet_id.clone(),
            label: "Bitcoin Mainnet".into(),
            chain_id: ChainId::Bitcoin,
            category: NetworkCategory::Mainnet,
            evm_chain_id: None,
        },
        BlockchainNetwork {
            id: testnet4_id.clone(),
            label: "Bitcoin Testnet4".into(),
            chain_id: ChainId::Bitcoin,
            category: NetworkCategory::Testnet,
            evm_chain_id: None,
        },
        BlockchainNetwork {
            id: signet_id.clone(),
            label: "Bitcoin Signet".into(),
            chain_id: ChainId::Bitcoin,
            category: NetworkCategory::Testnet,
            evm_chain_id: None,
        },
    ];

    let api_urls = build_api_urls(&mainnet_id, &testnet4_id, &signet_id);
    let explorer_urls = build_explorer_urls(&mainnet_id, &testnet4_id, &signet_id);

    let mut address_formats = HashMap::new();
    address_formats.insert(mainnet_id, AddressFormat::bitcoin_mainnet());
    address_formats.insert(testnet4_id, AddressFormat::bitcoin_testnet());
    address_formats.insert(signet_id, AddressFormat::bitcoin_testnet());

    BtcConfig {
        chain_id: ChainId::Bitcoin,
        native_asset: AssetInfo::btc(),
        derivation_path: dontyeet_crypto::derivation::paths::BITCOIN_SEGWIT.to_owned(),
        networks,
        api_urls,
        explorer_urls,
        address_formats,
    }
}

/// Construct Mempool.space REST API base URLs.
fn build_api_urls(
    mainnet: &NetworkId,
    testnet4: &NetworkId,
    signet: &NetworkId,
) -> HashMap<NetworkId, Vec<Url>> {
    let mut map = HashMap::new();

    // All URL parsing uses known-good literals, so parse errors are
    // programming bugs. We use a helper that returns an empty vec on
    // parse failure to avoid `unwrap()` in library code.
    map.insert(mainnet.clone(), parse_urls(&["https://mempool.space/api"]));
    map.insert(
        testnet4.clone(),
        parse_urls(&["https://mempool.space/testnet4/api"]),
    );
    map.insert(
        signet.clone(),
        parse_urls(&["https://mempool.space/signet/api"]),
    );
    map
}

/// Construct Mempool.space explorer URL templates.
fn build_explorer_urls(
    mainnet: &NetworkId,
    testnet4: &NetworkId,
    signet: &NetworkId,
) -> HashMap<NetworkId, ExplorerUrls> {
    let mut map = HashMap::new();
    map.insert(
        mainnet.clone(),
        ExplorerUrls::new(
            "https://mempool.space/address/{address}",
            "https://mempool.space/tx/{tx}",
        ),
    );
    map.insert(
        testnet4.clone(),
        ExplorerUrls::new(
            "https://mempool.space/testnet4/address/{address}",
            "https://mempool.space/testnet4/tx/{tx}",
        ),
    );
    map.insert(
        signet.clone(),
        ExplorerUrls::new(
            "https://mempool.space/signet/address/{address}",
            "https://mempool.space/signet/tx/{tx}",
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
        let cfg = default_btc_config();
        assert_eq!(cfg.networks.len(), 3);
    }

    #[test]
    fn mainnet_is_first_network() {
        let cfg = default_btc_config();
        assert_eq!(cfg.networks[0].id, NetworkId::new("bitcoin-mainnet"));
        assert_eq!(cfg.networks[0].category, NetworkCategory::Mainnet);
    }

    #[test]
    fn api_urls_populated() {
        let cfg = default_btc_config();
        let mainnet = NetworkId::new("bitcoin-mainnet");
        let urls = cfg.api_urls.get(&mainnet);
        assert!(urls.is_some());
        assert!(!urls.is_none_or(Vec::is_empty));
    }

    #[test]
    fn explorer_urls_populated() {
        let cfg = default_btc_config();
        let mainnet = NetworkId::new("bitcoin-mainnet");
        let explorer = cfg.explorer_urls.get(&mainnet);
        assert!(explorer.is_some());
    }

    #[test]
    fn address_formats_cover_every_network() {
        // Invariant: each declared network must have a matching format
        // entry. A missing format means address derivation will panic
        // for that network at runtime.
        let cfg = default_btc_config();
        for net in &cfg.networks {
            assert!(
                cfg.address_formats.contains_key(&net.id),
                "missing address_format for network {:?}",
                net.id,
            );
        }
    }

    #[test]
    fn bitcoin_mainnet_format_uses_canonical_bytes() {
        let f = AddressFormat::bitcoin_mainnet();
        assert_eq!(f.bech32_hrp, "bc");
        assert_eq!(f.base58_p2pkh_version, 0x00);
        assert_eq!(f.base58_p2sh_version, 0x05);
    }

    #[test]
    fn bitcoin_testnet_format_uses_canonical_bytes() {
        let f = AddressFormat::bitcoin_testnet();
        assert_eq!(f.bech32_hrp, "tb");
        assert_eq!(f.base58_p2pkh_version, 0x6F);
        assert_eq!(f.base58_p2sh_version, 0xC4);
    }
}

// Rust guideline compliant 2026-05-02
