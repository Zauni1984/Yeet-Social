//! Kadena chain plugin — bundles all Kadena capabilities into a single
//! [`ChainPlugin`] implementation.
//!
//! ## Status
//!
//! All standard wallet operations (balance, address derivation, RPC
//! health, fee estimation, send) target chain `0` of the community
//! Chainweb endpoint. Sending uses a `coin.transfer` Pact command signed
//! with Ed25519; see [`crate::signing`] for details. Cross-chain
//! transfers and arbitrary chain selection are out of scope for now.

use std::any::Any;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use url::Url;

use dontyeet_primitives::asset::AssetInfo;
use dontyeet_primitives::chain::{ChainId, NetworkCategory, NetworkId};
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::network::{BlockchainNetwork, ExplorerUrls};
use dontyeet_primitives::secret::Seed;
use dontyeet_primitives::traits::{ChainPlugin, KeyDeriver, TransactionBroadcaster};
use dontyeet_primitives::transaction::TxHash;
use dontyeet_primitives::{Address, Amount};

use dontyeet_chain::FnSigner;
use dontyeet_network::NetworkCatalog;

use crate::balance::KadenaBalanceFetcher;
use crate::broadcast::KadenaBroadcaster;
use crate::config::KadenaConfig;
use crate::fees::KadenaFeeEstimator;
use crate::keys::{KadenaAddressEncoder, KadenaKeyDeriver};

/// A complete Kadena Chainweb chain integration.
///
/// Constructed via the [`kadena_plugin`] factory function.
pub struct KadenaChainPlugin {
    config: KadenaConfig,
    key_deriver: KadenaKeyDeriver,
    address_encoder: KadenaAddressEncoder,
    fee_estimator: KadenaFeeEstimator,
    balance_fetcher: KadenaBalanceFetcher,
    tx_signer: FnSigner,
    broadcaster: KadenaBroadcaster,
    network_catalog: NetworkCatalog,
}

impl KadenaChainPlugin {
    /// Create a new Kadena chain plugin from the given configuration.
    #[must_use]
    pub fn new(config: KadenaConfig) -> Self {
        let key_deriver = KadenaKeyDeriver::new(&config);
        let fee_estimator = KadenaFeeEstimator::new(&config.api_urls);
        let balance_fetcher = KadenaBalanceFetcher::new(&config.api_urls, &config.network_versions);
        let broadcaster = KadenaBroadcaster::new(&config.api_urls, &config.network_versions);
        let network_catalog = NetworkCatalog::new(
            config.networks.clone(),
            config.explorer_urls.clone(),
            config.api_urls.clone(),
        );

        Self {
            config,
            key_deriver,
            address_encoder: KadenaAddressEncoder,
            fee_estimator,
            balance_fetcher,
            tx_signer: FnSigner::new(|msg, key| {
                crate::signing::sign_hash(msg, key).map(|sig| sig.to_vec())
            }),
            broadcaster,
            network_catalog,
        }
    }

    /// Borrow the underlying configuration.
    #[must_use]
    pub fn config(&self) -> &KadenaConfig {
        &self.config
    }
}

#[async_trait]
impl ChainPlugin for KadenaChainPlugin {
    dontyeet_chain::chain_plugin_accessors! {
        config_field    = config,
        key_deriver     = key_deriver,
        address_encoder = address_encoder,
        balance_fetcher = balance_fetcher,
        fee_estimator   = fee_estimator,
        signer          = tx_signer,
        broadcaster     = broadcaster,
        network         = network_catalog,
    }

    /// Pact transactions are constructed inline by [`Self::send_transfer`];
    /// there is no separate type-erased builder to expose. Returning the
    /// fee estimator keeps the existing trait method total without
    /// claiming to provide a real builder.
    fn tx_builder_any(&self) -> &dyn Any {
        &self.fee_estimator
    }

    /// Build, sign, and broadcast a `coin.transfer` Pact transaction.
    ///
    /// Targets chain `0` of the configured Chainweb network. The sender
    /// account is derived from `seed`; the recipient must be a `k:`
    /// address (vanity / `w:` accounts are rejected — see
    /// [`crate::signing`]).
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Validation` for malformed addresses, zero
    /// amounts, or arithmetic overflow when stamping the creation time;
    /// `DontYeetWalletError::Crypto` if signing fails; `DontYeetWalletError::Network`
    /// or `DontYeetWalletError::NotFound` from the broadcaster.
    async fn send_transfer(
        &self,
        seed: &Seed,
        to: &Address,
        amount: &Amount,
        network: &NetworkId,
    ) -> Result<TxHash> {
        let network_version =
            self.config.network_versions.get(network).ok_or_else(|| {
                DontYeetWalletError::NotFound(format!("no network version for {network}"))
            })?;

        let keypair = self.key_deriver.derive_keypair(seed, network)?;
        let private_key = keypair.private_key().ok_or_else(|| {
            DontYeetWalletError::Chain("no private key derived for Kadena keypair".into())
        })?;

        let creation_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| DontYeetWalletError::Validation(format!("system clock before epoch: {e}")))?
            .as_secs();

        let signed = crate::signing::build_signed_transfer(
            network_version,
            &keypair.address,
            to,
            amount,
            private_key,
            creation_time,
        )?;

        self.broadcaster.broadcast(&signed, network).await
    }
}

// ---------------------------------------------------------------------------
// Factory function
// ---------------------------------------------------------------------------

/// Create a [`KadenaChainPlugin`] configured for legacy mainnet, community
/// mainnet, and community testnet.
///
/// Networks:
/// - `kadena-community` — Kadena Community mainnet (kda-community fork, Nov 2025)
/// - `kadena-legacy`    — Kadena Legacy mainnet (original chain, stopped Nov 2025)
/// - `kadena-testnet`   — Kadena Community testnet (testnet05)
#[must_use]
pub fn kadena_plugin() -> KadenaChainPlugin {
    let community_id = NetworkId::new("kadena-community");
    let legacy_id = NetworkId::new("kadena-legacy");
    let testnet_id = NetworkId::new("kadena-testnet");

    let networks = vec![
        BlockchainNetwork {
            id: community_id.clone(),
            label: "Kadena Community".into(),
            chain_id: ChainId::Kadena,
            category: NetworkCategory::Mainnet,
            evm_chain_id: None,
        },
        BlockchainNetwork {
            id: legacy_id.clone(),
            label: "Kadena Legacy".into(),
            chain_id: ChainId::Kadena,
            category: NetworkCategory::Mainnet,
            evm_chain_id: None,
        },
        BlockchainNetwork {
            id: testnet_id.clone(),
            label: "Kadena Community Testnet".into(),
            chain_id: ChainId::Kadena,
            category: NetworkCategory::Testnet,
            evm_chain_id: None,
        },
    ];

    let mut api_urls = HashMap::new();
    api_urls.insert(
        community_id.clone(),
        vec![parse_url("https://api.chainweb-community.org")],
    );
    api_urls.insert(
        legacy_id.clone(),
        vec![parse_url("https://api.chainweb.com")],
    );
    api_urls.insert(
        testnet_id.clone(),
        vec![parse_url("https://api.testnet.chainweb-community.org")],
    );

    let mut network_versions = HashMap::new();
    network_versions.insert(community_id.clone(), "mainnet01".into());
    network_versions.insert(legacy_id.clone(), "mainnet01".into());
    network_versions.insert(testnet_id.clone(), "testnet05".into());

    let mut explorer_urls = HashMap::new();
    explorer_urls.insert(
        community_id,
        ExplorerUrls::new(
            "https://explorer.chainweb-community.org/mainnet/account/{address}",
            "https://explorer.chainweb-community.org/mainnet/tx/{tx}",
        ),
    );
    explorer_urls.insert(
        legacy_id,
        ExplorerUrls::new(
            "https://explorer.chainweb.com/mainnet/account/{address}",
            "https://explorer.chainweb.com/mainnet/tx/{tx}",
        ),
    );
    explorer_urls.insert(
        testnet_id,
        ExplorerUrls::new(
            "https://explorer.chainweb-community.org/testnet/account/{address}",
            "https://explorer.chainweb-community.org/testnet/tx/{tx}",
        ),
    );

    let config = KadenaConfig {
        chain_id: ChainId::Kadena,
        native_asset: AssetInfo::kda(),
        derivation_path: dontyeet_crypto::paths::KADENA.into(),
        networks,
        api_urls,
        network_versions,
        explorer_urls,
    };

    KadenaChainPlugin::new(config)
}

/// Parse a URL string that is known to be valid at compile time.
fn parse_url(s: &str) -> Url {
    s.parse().expect("hardcoded URL must be valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_produces_correct_chain_id() {
        let plugin = kadena_plugin();
        assert_eq!(plugin.chain_id(), &ChainId::Kadena);
    }

    #[test]
    fn factory_has_three_networks() {
        let plugin = kadena_plugin();
        let networks = plugin.network_provider().networks();
        assert_eq!(networks.len(), 3);
        assert_eq!(networks[0].label, "Kadena Community");
        assert_eq!(networks[1].label, "Kadena Legacy");
        assert_eq!(networks[2].label, "Kadena Community Testnet");
    }

    #[test]
    fn native_asset_is_kda() {
        let plugin = kadena_plugin();
        let asset = plugin.native_asset();
        assert_eq!(asset.symbol, "KDA");
        assert_eq!(asset.decimals, 12);
    }

    #[test]
    fn default_network_is_mainnet() {
        let plugin = kadena_plugin();
        let default = plugin.network_provider().default_network();
        assert_eq!(default.category, NetworkCategory::Mainnet);
    }
}

// Rust guideline compliant 2026-05-02
