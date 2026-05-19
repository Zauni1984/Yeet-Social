//! Kaspa chain plugin — bundles all Kaspa capabilities into a single
//! [`ChainPlugin`] implementation.

use std::any::Any;
use std::collections::HashMap;

use async_trait::async_trait;
use url::Url;

use dontyeet_primitives::asset::AssetInfo;
use dontyeet_primitives::chain::{ChainId, NetworkCategory, NetworkId};
use dontyeet_primitives::network::{BlockchainNetwork, ExplorerUrls};
use dontyeet_primitives::secret::Seed;
use dontyeet_primitives::traits::{ChainPlugin, FeeEstimator, KeyDeriver, TransactionBroadcaster};
use dontyeet_primitives::transaction::{
    RpcHealthResult, StandardizedFee, StandardizedFeeTier, TxHash,
};
use dontyeet_primitives::{Address, Amount, DontYeetWalletError, Result};

use dontyeet_chain::FnSigner;
use dontyeet_network::{HttpClient, NetworkCatalog, ReqwestClient};

use crate::balance::KaspaBalanceFetcher;
use crate::broadcast::KaspaBroadcaster;
use crate::config::KaspaConfig;
use crate::fees::KaspaFeeEstimator;
use crate::history::KaspaHistoryFetcher;
use crate::keys::{KaspaAddressEncoder, KaspaKeyDeriver};

/// A complete Kaspa `BlockDAG` chain integration.
///
/// Constructed via the [`kaspa_plugin`] factory function.
pub struct KaspaChainPlugin {
    config: KaspaConfig,
    key_deriver: KaspaKeyDeriver,
    address_encoder: KaspaAddressEncoder,
    fee_estimator: KaspaFeeEstimator,
    balance_fetcher: KaspaBalanceFetcher,
    tx_signer: FnSigner,
    broadcaster: KaspaBroadcaster,
    network_catalog: NetworkCatalog,
    history_fetcher: KaspaHistoryFetcher,
}

impl KaspaChainPlugin {
    /// Create a new Kaspa chain plugin from the given configuration.
    #[must_use]
    pub fn new(config: KaspaConfig) -> Self {
        let key_deriver = KaspaKeyDeriver::new(&config);
        let fee_estimator = KaspaFeeEstimator::new(&config.api_urls);
        let balance_fetcher = KaspaBalanceFetcher::new(&config.api_urls);
        let broadcaster = KaspaBroadcaster::new(&config.api_urls);
        let network_catalog = NetworkCatalog::new(
            config.networks.clone(),
            config.explorer_urls.clone(),
            config.api_urls.clone(),
        );
        let history_fetcher = KaspaHistoryFetcher::new(&config.api_urls);

        Self {
            config,
            key_deriver,
            address_encoder: KaspaAddressEncoder,
            fee_estimator,
            balance_fetcher,
            tx_signer: FnSigner::new(crate::signing::sign_input),
            broadcaster,
            network_catalog,
            history_fetcher,
        }
    }

    /// Borrow the underlying configuration.
    #[must_use]
    pub fn config(&self) -> &KaspaConfig {
        &self.config
    }
}

#[async_trait]
impl ChainPlugin for KaspaChainPlugin {
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

    fn tx_builder_any(&self) -> &dyn Any {
        &self.fee_estimator // placeholder — UTXO tx builder TBD
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "elapsed health-check time fits in u64 ms; network timeouts cap it well below u128 saturation"
    )]
    async fn check_rpc_health(&self, network: &NetworkId) -> Result<RpcHealthResult> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct BlueScoreResponse {
            blue_score: String,
        }

        let Some(urls) = self.config.api_urls.get(network) else {
            return Ok(RpcHealthResult {
                reachable: false,
                response_time_ms: 0,
                latest_block: None,
                error: Some(format!("no API URLs for {network}")),
            });
        };

        let Some(base) = urls.first() else {
            return Ok(RpcHealthResult {
                reachable: false,
                response_time_ms: 0,
                latest_block: None,
                error: Some("API URL list is empty".into()),
            });
        };

        let Ok(score_url) = base.join("/info/virtual-chain-blue-score") else {
            return Ok(RpcHealthResult {
                reachable: false,
                response_time_ms: 0,
                latest_block: None,
                error: Some("URL join error".into()),
            });
        };

        let start = std::time::Instant::now();
        let result = async {
            let client =
                ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
            let response = client
                .get(&score_url)
                .await
                .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
            let resp: BlueScoreResponse = response
                .json()
                .map_err(|e| DontYeetWalletError::Network(format!("blue score parse error: {e}")))?;
            let score: u64 = resp
                .blue_score
                .parse()
                .map_err(|e| DontYeetWalletError::Network(format!("blue score value error: {e}")))?;
            Ok::<u64, DontYeetWalletError>(score)
        }
        .await;
        let elapsed = start.elapsed().as_millis() as u64;

        match result {
            Ok(score) => Ok(RpcHealthResult {
                reachable: true,
                response_time_ms: elapsed,
                latest_block: Some(score),
                error: None,
            }),
            Err(e) => Ok(RpcHealthResult {
                reachable: false,
                response_time_ms: elapsed,
                latest_block: None,
                error: Some(e.to_string()),
            }),
        }
    }

    async fn estimate_fee_display(&self, network: &NetworkId) -> Result<StandardizedFeeTier> {
        /// Format sompi as a human-readable KAS string.
        ///
        /// 1 KAS = 100,000,000 sompi (8 decimal places).
        fn kas_label(sompi: u64) -> String {
            let whole = sompi / 100_000_000;
            let frac = sompi % 100_000_000;
            format!("~{whole}.{frac:08} KAS")
        }

        let tiers = self.fee_estimator.estimate_fees(network).await?;

        let slow_sompi = tiers.slow.total_sompi()?;
        let std_sompi = tiers.standard.total_sompi()?;
        let fast_sompi = tiers.fast.total_sompi()?;

        Ok(StandardizedFeeTier {
            slow: StandardizedFee {
                label: kas_label(slow_sompi),
                native_amount: slow_sompi.to_string(),
            },
            standard: StandardizedFee {
                label: kas_label(std_sompi),
                native_amount: std_sompi.to_string(),
            },
            fast: StandardizedFee {
                label: kas_label(fast_sompi),
                native_amount: fast_sompi.to_string(),
            },
        })
    }

    async fn send_transfer(
        &self,
        seed: &Seed,
        to: &Address,
        amount: &Amount,
        network: &NetworkId,
    ) -> Result<TxHash> {
        let keypair = self.key_deriver.derive_keypair(seed, network)?;
        let private_key = keypair
            .private_key()
            .ok_or_else(|| DontYeetWalletError::Chain("no private key derived".into()))?;

        let signed_tx = crate::transfer::build_signed_transfer(
            &self.config.api_urls,
            &keypair.address,
            to,
            amount,
            private_key,
            network,
        )
        .await?;

        self.broadcaster.broadcast(&signed_tx, network).await
    }

    async fn fetch_history(
        &self,
        address: &Address,
        network: &NetworkId,
        limit: usize,
    ) -> Result<Vec<dontyeet_primitives::transaction::TxHistoryItem>> {
        self.history_fetcher
            .fetch(address.as_str(), network, limit)
            .await
    }
}

// ---------------------------------------------------------------------------
// Factory function
// ---------------------------------------------------------------------------

/// Create a [`KaspaChainPlugin`] configured for mainnet and testnet.
///
/// Networks:
/// - `kaspa-mainnet` — Kaspa mainnet
/// - `kaspa-testnet` — Kaspa testnet (testnet-11)
#[must_use]
pub fn kaspa_plugin() -> KaspaChainPlugin {
    let mainnet_id = NetworkId::new("kaspa-mainnet");
    let testnet_id = NetworkId::new("kaspa-testnet");

    let networks = vec![
        BlockchainNetwork {
            id: mainnet_id.clone(),
            label: "Kaspa Mainnet".into(),
            chain_id: ChainId::Kaspa,
            category: NetworkCategory::Mainnet,
            evm_chain_id: None,
        },
        BlockchainNetwork {
            id: testnet_id.clone(),
            label: "Kaspa Testnet".into(),
            chain_id: ChainId::Kaspa,
            category: NetworkCategory::Testnet,
            evm_chain_id: None,
        },
    ];

    let mut api_urls = HashMap::new();
    api_urls.insert(mainnet_id.clone(), vec![parse_url("https://api.kaspa.org")]);
    api_urls.insert(
        testnet_id.clone(),
        vec![parse_url("https://api-tn.kaspa.org")],
    );

    let mut explorer_urls = HashMap::new();
    explorer_urls.insert(
        mainnet_id,
        ExplorerUrls::new(
            "https://explorer.kaspa.org/addresses/{address}",
            "https://explorer.kaspa.org/txs/{tx}",
        ),
    );
    explorer_urls.insert(
        testnet_id,
        ExplorerUrls::new(
            "https://explorer-tn.kaspa.org/addresses/{address}",
            "https://explorer-tn.kaspa.org/txs/{tx}",
        ),
    );

    let config = KaspaConfig {
        chain_id: ChainId::Kaspa,
        native_asset: AssetInfo::kas(),
        derivation_path: dontyeet_crypto::paths::KASPA.into(),
        networks,
        api_urls,
        explorer_urls,
    };

    KaspaChainPlugin::new(config)
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
        let plugin = kaspa_plugin();
        assert_eq!(plugin.chain_id(), &ChainId::Kaspa);
    }

    #[test]
    fn factory_has_two_networks() {
        let plugin = kaspa_plugin();
        let networks = plugin.network_provider().networks();
        assert_eq!(networks.len(), 2);
        assert_eq!(networks[0].label, "Kaspa Mainnet");
        assert_eq!(networks[1].label, "Kaspa Testnet");
    }

    #[test]
    fn native_asset_is_kas() {
        let plugin = kaspa_plugin();
        let asset = plugin.native_asset();
        assert_eq!(asset.symbol, "KAS");
        assert_eq!(asset.decimals, 8);
    }

    #[test]
    fn default_network_is_mainnet() {
        let plugin = kaspa_plugin();
        let default = plugin.network_provider().default_network();
        assert_eq!(default.category, NetworkCategory::Mainnet);
    }
}

// Rust guideline compliant 2026-05-02
