//! Privacy layer — optional Tor routing and RPC endpoint rotation.
//!
//! Without privacy mode, RPC providers can correlate your IP address with
//! every wallet address you query, building a complete profile of your
//! holdings.
//!
//! ## Tor mode
//!
//! Routes all RPC traffic through a SOCKS5 proxy (typically Tor on
//! `127.0.0.1:9050`).  Hides your IP from node operators.
//!
//! ## Endpoint rotation
//!
//! Distributes queries across multiple RPC providers in round-robin so no
//! single provider sees all your addresses.
//!
//! ## Future: local node
//!
//! Running your own node eliminates the privacy problem entirely — queries
//! never leave your machine.  This is planned as a later feature.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use url::Url;

/// How to route requests for privacy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum PrivacyMode {
    /// Direct connection (default, fastest, no privacy).
    #[default]
    Direct,

    /// Route through a SOCKS5 proxy (e.g. Tor).
    Proxy {
        /// SOCKS5 proxy URL, e.g. `"socks5://127.0.0.1:9050"` for Tor.
        proxy_url: String,
    },
}

impl PrivacyMode {
    /// Create a Tor privacy mode using the default Tor SOCKS5 port.
    #[must_use]
    pub fn tor() -> Self {
        Self::Proxy {
            proxy_url: "socks5://127.0.0.1:9050".into(),
        }
    }

    /// Create a Tor privacy mode with a custom SOCKS5 address.
    #[must_use]
    pub fn tor_custom(proxy_url: impl Into<String>) -> Self {
        Self::Proxy {
            proxy_url: proxy_url.into(),
        }
    }

    /// Whether this mode routes through a proxy.
    #[must_use]
    pub const fn is_proxied(&self) -> bool {
        matches!(self, Self::Proxy { .. })
    }
}

/// Distributes queries across multiple RPC endpoints in round-robin.
///
/// No single provider sees all your address queries.
pub struct EndpointRotator {
    endpoints: Vec<Url>,
    index: AtomicUsize,
}

impl EndpointRotator {
    /// Create a rotator from a list of endpoint URLs.
    ///
    /// # Panics
    /// Panics if `endpoints` is empty.
    #[must_use]
    pub fn new(endpoints: Vec<Url>) -> Self {
        assert!(!endpoints.is_empty(), "endpoint list must not be empty");
        Self {
            endpoints,
            index: AtomicUsize::new(0),
        }
    }

    /// Get the next endpoint in round-robin order.
    #[must_use]
    pub fn next(&self) -> &Url {
        let idx = self.index.fetch_add(1, Ordering::Relaxed) % self.endpoints.len();
        &self.endpoints[idx]
    }

    /// Number of endpoints in the rotation.
    #[must_use]
    pub fn len(&self) -> usize {
        self.endpoints.len()
    }

    /// Whether the rotator has no endpoints (always false after construction).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.endpoints.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_mode_default_is_direct() {
        assert!(!PrivacyMode::default().is_proxied());
    }

    #[test]
    fn tor_mode_is_proxied() {
        assert!(PrivacyMode::tor().is_proxied());
    }

    #[test]
    fn endpoint_rotation_round_robin() {
        let urls: Vec<Url> = vec![
            "https://rpc1.example.com".parse().expect("url"),
            "https://rpc2.example.com".parse().expect("url"),
            "https://rpc3.example.com".parse().expect("url"),
        ];
        let rotator = EndpointRotator::new(urls);

        let first = rotator.next().to_string();
        let second = rotator.next().to_string();
        let third = rotator.next().to_string();
        let fourth = rotator.next().to_string();

        // Should cycle: 1, 2, 3, 1
        assert_eq!(first, "https://rpc1.example.com/");
        assert_eq!(second, "https://rpc2.example.com/");
        assert_eq!(third, "https://rpc3.example.com/");
        assert_eq!(fourth, "https://rpc1.example.com/");
    }

    #[test]
    fn single_endpoint_always_returns_same() {
        let urls: Vec<Url> = vec!["https://only.example.com".parse().expect("url")];
        let rotator = EndpointRotator::new(urls);
        assert_eq!(rotator.next().as_str(), "https://only.example.com/");
        assert_eq!(rotator.next().as_str(), "https://only.example.com/");
    }
}

// Rust guideline compliant 2026-05-02
