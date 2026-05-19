//! Catalog of supported networks with their explorer and API endpoints.
//!
//! Every chain integration crate needs a concrete type that exposes its
//! supported networks, explorer URLs, and API endpoints. Before this module
//! existed each crate hand-rolled its own struct (`BtcNetworkProvider`,
//! `SolNetworkProvider`, ...) — all with byte-identical method bodies that
//! only differed in the type name.
//!
//! [`NetworkCatalog`] is that shared implementation. A chain plugin
//! constructs one from its config and stores it (typically wrapped in
//! `Arc<dyn NetworkProvider>` and `Arc<dyn RpcEndpointProvider>`).
//!
//! The catalog data does not change after construction. Hot-reloading
//! endpoints is not in scope here — replace the catalog instance instead.

use std::collections::HashMap;

use url::Url;

use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::network::{BlockchainNetwork, ExplorerUrls};
use dontyeet_primitives::traits::{NetworkProvider, RpcEndpointProvider};

/// Catalog of supported networks with explorer and API URL maps.
#[derive(Debug, Clone)]
pub struct NetworkCatalog {
    networks: Vec<BlockchainNetwork>,
    explorer_urls: HashMap<NetworkId, ExplorerUrls>,
    api_urls: HashMap<NetworkId, Vec<Url>>,
}

impl NetworkCatalog {
    /// Construct a catalog from owned network metadata and URL maps.
    ///
    /// `networks` is the supported network list; the first entry is treated
    /// as the default network. `explorer_urls` and `api_urls` should both be
    /// keyed by the same [`NetworkId`]s that appear in `networks`, but
    /// missing entries are tolerated and surface as `NotFound` at lookup
    /// time rather than at construction.
    #[must_use]
    pub fn new(
        networks: Vec<BlockchainNetwork>,
        explorer_urls: HashMap<NetworkId, ExplorerUrls>,
        api_urls: HashMap<NetworkId, Vec<Url>>,
    ) -> Self {
        Self {
            networks,
            explorer_urls,
            api_urls,
        }
    }

    /// All supported networks, in declaration order.
    #[must_use]
    pub fn networks(&self) -> &[BlockchainNetwork] {
        &self.networks
    }

    /// The default network (first entry in the configured list).
    ///
    /// # Panics
    /// Panics if the catalog was constructed with an empty `networks`
    /// list. This matches the behavior of the per-chain providers this
    /// type replaces, where an empty network list is a configuration bug
    /// the application is expected to catch at startup.
    #[must_use]
    pub fn default_network(&self) -> &BlockchainNetwork {
        &self.networks[0]
    }

    /// Explorer URLs for `network`.
    ///
    /// # Errors
    /// Returns [`DontYeetWalletError::NotFound`] if no explorer URLs are
    /// configured for `network`.
    pub fn explorer_urls(&self, network: &NetworkId) -> Result<ExplorerUrls> {
        self.explorer_urls
            .get(network)
            .cloned()
            .ok_or_else(|| DontYeetWalletError::NotFound(format!("no explorer URLs for {network}")))
    }

    /// Ordered API URLs for `network` (first = preferred).
    ///
    /// # Errors
    /// Returns [`DontYeetWalletError::NotFound`] if no API URLs are configured
    /// for `network`.
    pub fn rpc_urls(&self, network: &NetworkId) -> Result<Vec<Url>> {
        self.api_urls
            .get(network)
            .cloned()
            .ok_or_else(|| DontYeetWalletError::NotFound(format!("no API URLs for {network}")))
    }
}

impl NetworkProvider for NetworkCatalog {
    fn networks(&self) -> &[BlockchainNetwork] {
        Self::networks(self)
    }

    fn default_network(&self) -> &BlockchainNetwork {
        Self::default_network(self)
    }

    fn explorer_urls(&self, network: &NetworkId) -> Result<ExplorerUrls> {
        Self::explorer_urls(self, network)
    }
}

impl RpcEndpointProvider for NetworkCatalog {
    fn rpc_urls(&self, network: &NetworkId) -> Result<Vec<Url>> {
        Self::rpc_urls(self, network)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dontyeet_primitives::chain::{ChainId, NetworkCategory};

    fn sample_network_id(label: &str) -> NetworkId {
        NetworkId::new(format!("test-{label}"))
    }

    fn sample_network(label: &str) -> BlockchainNetwork {
        BlockchainNetwork {
            id: sample_network_id(label),
            label: label.to_owned(),
            chain_id: ChainId::Other("test".to_owned()),
            category: NetworkCategory::Mainnet,
            evm_chain_id: None,
        }
    }

    fn sample_explorer() -> ExplorerUrls {
        ExplorerUrls::new(
            "https://example.test/address/{address}",
            "https://example.test/tx/{tx}",
        )
    }

    fn sample_url() -> Url {
        Url::parse("https://example.test/api").expect("static URL parses")
    }

    fn make_catalog() -> NetworkCatalog {
        let mainnet = sample_network("main");
        let testnet = sample_network("test");
        let mut explorers = HashMap::new();
        explorers.insert(mainnet.id.clone(), sample_explorer());
        let mut apis = HashMap::new();
        apis.insert(mainnet.id.clone(), vec![sample_url()]);
        NetworkCatalog::new(vec![mainnet, testnet], explorers, apis)
    }

    #[test]
    fn networks_returns_in_order() {
        let catalog = make_catalog();
        let names: Vec<&str> = catalog
            .networks()
            .iter()
            .map(|n| n.label.as_str())
            .collect();
        assert_eq!(names, vec!["main", "test"]);
    }

    #[test]
    fn default_network_is_first() {
        let catalog = make_catalog();
        assert_eq!(catalog.default_network().label, "main");
    }

    #[test]
    fn explorer_urls_present_returns_clone() {
        let catalog = make_catalog();
        let urls = catalog
            .explorer_urls(&sample_network_id("main"))
            .expect("explorer urls present");
        assert!(urls.address_url.contains("{address}"));
    }

    #[test]
    fn explorer_urls_missing_returns_not_found() {
        let catalog = make_catalog();
        let err = catalog
            .explorer_urls(&sample_network_id("test"))
            .expect_err("explorer urls missing");
        assert!(matches!(err, DontYeetWalletError::NotFound(_)));
    }

    #[test]
    fn rpc_urls_present_returns_clone() {
        let catalog = make_catalog();
        let urls = catalog
            .rpc_urls(&sample_network_id("main"))
            .expect("rpc urls present");
        assert_eq!(urls.len(), 1);
    }

    #[test]
    fn rpc_urls_missing_returns_not_found() {
        let catalog = make_catalog();
        let err = catalog
            .rpc_urls(&sample_network_id("test"))
            .expect_err("rpc urls missing");
        assert!(matches!(err, DontYeetWalletError::NotFound(_)));
    }

    #[test]
    fn trait_dispatch_matches_inherent() {
        let catalog = make_catalog();
        let trait_obj: &dyn NetworkProvider = &catalog;
        assert_eq!(trait_obj.networks().len(), 2);
        assert_eq!(trait_obj.default_network().label, "main");
        let rpc_obj: &dyn RpcEndpointProvider = &catalog;
        assert!(rpc_obj.rpc_urls(&sample_network_id("main")).is_ok());
    }
}

// Rust guideline compliant 2026-05-02
