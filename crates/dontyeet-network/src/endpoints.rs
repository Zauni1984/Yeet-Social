//! Per-network endpoint URL lookup with `NotFound` error semantics.
//!
//! Every chain crate stores a `HashMap<NetworkId, Vec<Url>>` of API
//! endpoints and follows the same dance to pick a URL: look up the
//! network, take the first URL, return [`DontYeetWalletError::NotFound`] with
//! one of two specific messages if either step fails. [`Endpoints`] wraps
//! that map and exposes the lookup as a single call.
//!
//! Endpoint rotation across the full URL list is intentionally out of
//! scope here; see [`crate::privacy::EndpointRotator`] for the planned
//! integration point.

use std::collections::HashMap;

use url::Url;

use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};

/// Per-network API endpoint URLs with primary-first ordering.
#[derive(Debug, Clone, Default)]
pub struct Endpoints {
    api_urls: HashMap<NetworkId, Vec<Url>>,
}

impl Endpoints {
    /// Construct from an owned map of `network -> ordered URL list`.
    #[must_use]
    pub fn new(api_urls: HashMap<NetworkId, Vec<Url>>) -> Self {
        Self { api_urls }
    }

    /// The preferred URL for `network` (first entry in the list).
    ///
    /// # Errors
    /// Returns [`DontYeetWalletError::NotFound`] with `"no API URLs for {network}"`
    /// if `network` has no entry, or `"API URL list is empty"` if the
    /// entry exists but is an empty list.
    pub fn primary(&self, network: &NetworkId) -> Result<&Url> {
        let urls = self
            .api_urls
            .get(network)
            .ok_or_else(|| DontYeetWalletError::NotFound(format!("no API URLs for {network}")))?;
        urls.first()
            .ok_or_else(|| DontYeetWalletError::NotFound("API URL list is empty".into()))
    }

    /// All URLs for `network`, in declaration order.
    ///
    /// Useful for endpoint rotation or multi-endpoint health checks.
    ///
    /// # Errors
    /// Returns [`DontYeetWalletError::NotFound`] if `network` has no entry.
    /// An empty list is returned as `Ok(&[])` and is *not* an error here;
    /// callers that require at least one URL should use [`Self::primary`].
    pub fn all(&self, network: &NetworkId) -> Result<&[Url]> {
        self.api_urls
            .get(network)
            .map(Vec::as_slice)
            .ok_or_else(|| DontYeetWalletError::NotFound(format!("no API URLs for {network}")))
    }

    /// Whether any URLs are configured for `network`.
    #[must_use]
    pub fn contains(&self, network: &NetworkId) -> bool {
        self.api_urls
            .get(network)
            .is_some_and(|urls| !urls.is_empty())
    }
}

impl From<HashMap<NetworkId, Vec<Url>>> for Endpoints {
    fn from(api_urls: HashMap<NetworkId, Vec<Url>>) -> Self {
        Self::new(api_urls)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nid(label: &str) -> NetworkId {
        NetworkId::new(format!("test-{label}"))
    }

    fn url(s: &str) -> Url {
        Url::parse(s).expect("static URL parses")
    }

    #[test]
    fn primary_returns_first_url() {
        let main = nid("main");
        let mut map = HashMap::new();
        map.insert(
            main.clone(),
            vec![
                url("https://primary.test/"),
                url("https://fallback.test/"),
            ],
        );
        let endpoints = Endpoints::new(map);
        assert_eq!(
            endpoints.primary(&main).expect("primary present").as_str(),
            "https://primary.test/"
        );
    }

    #[test]
    fn primary_missing_network_is_not_found() {
        let endpoints = Endpoints::default();
        let err = endpoints
            .primary(&nid("absent"))
            .expect_err("missing network");
        assert!(matches!(err, DontYeetWalletError::NotFound(ref m) if m.contains("no API URLs for")));
    }

    #[test]
    fn primary_empty_list_is_not_found() {
        let main = nid("main");
        let mut map = HashMap::new();
        map.insert(main.clone(), Vec::new());
        let endpoints = Endpoints::new(map);
        let err = endpoints.primary(&main).expect_err("empty url list");
        assert!(matches!(err, DontYeetWalletError::NotFound(ref m) if m == "API URL list is empty"));
    }

    #[test]
    fn all_returns_full_slice() {
        let main = nid("main");
        let mut map = HashMap::new();
        map.insert(
            main.clone(),
            vec![url("https://a.test/"), url("https://b.test/")],
        );
        let endpoints = Endpoints::new(map);
        let urls = endpoints.all(&main).expect("urls present");
        assert_eq!(urls.len(), 2);
    }

    #[test]
    fn all_missing_network_is_not_found() {
        let endpoints = Endpoints::default();
        let err = endpoints.all(&nid("absent")).expect_err("missing network");
        assert!(matches!(err, DontYeetWalletError::NotFound(_)));
    }

    #[test]
    fn contains_reflects_population() {
        let main = nid("main");
        let empty = nid("empty");
        let mut map = HashMap::new();
        map.insert(main.clone(), vec![url("https://a.test/")]);
        map.insert(empty.clone(), Vec::new());
        let endpoints = Endpoints::new(map);
        assert!(endpoints.contains(&main));
        assert!(!endpoints.contains(&empty));
        assert!(!endpoints.contains(&nid("absent")));
    }

    #[test]
    fn from_hashmap_works() {
        let mut map = HashMap::new();
        map.insert(nid("main"), vec![url("https://a.test/")]);
        let endpoints: Endpoints = map.into();
        assert!(endpoints.contains(&nid("main")));
    }
}

// Rust guideline compliant 2026-05-02
