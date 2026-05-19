//! Kaspa native balance fetching.
//!
//! Queries the Kaspa REST API to retrieve the KAS balance in SOMPI
//! (1 KAS = 100,000,000 SOMPI).

use std::collections::HashMap;

use async_trait::async_trait;
use serde::Deserialize;
use url::Url;

use dontyeet_network::{Endpoints, encode_segment};
use dontyeet_primitives::address::Address;
use dontyeet_primitives::amount::Amount;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::Result;
use dontyeet_primitives::traits::BalanceFetcher;

use crate::rest;

/// KAS has 8 decimal places.
const KAS_DECIMALS: u8 = 8;

/// JSON response from `GET /addresses/{address}/balance`.
#[derive(Debug, Deserialize)]
struct BalanceResponse {
    /// Balance in SOMPI.
    balance: u64,
}

/// Fetches native KAS balances via the Kaspa REST API.
pub struct KaspaBalanceFetcher {
    endpoints: Endpoints,
}

impl KaspaBalanceFetcher {
    /// Create a balance fetcher from the configured API URLs.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            endpoints: Endpoints::new(api_urls.clone()),
        }
    }
}

#[async_trait]
impl BalanceFetcher for KaspaBalanceFetcher {
    /// Fetch the native KAS balance for an address.
    ///
    /// Calls `GET {api}/addresses/{address}/balance` and returns the
    /// balance as an [`Amount`] with 8 decimal places.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the API call fails, or
    /// `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn fetch_balance(&self, address: &Address, network: &NetworkId) -> Result<Amount> {
        let urls = self.endpoints.all(network)?;

        let path = format!("addresses/{}/balance", encode_segment(address.as_str()));
        let body: BalanceResponse = rest::rest_get(urls, &path).await?;

        Ok(Amount::from_raw(u128::from(body.balance), KAS_DECIMALS))
    }
}

// Rust guideline compliant 2026-05-02
