//! Cardano balance fetching via Blockfrost REST API.
//!
//! Queries `GET /addresses/{addr}` and extracts `amount[].quantity`
//! for the native `lovelace` unit.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::Deserialize;
use url::Url;

use dontyeet_network::{Endpoints, HttpClient, ReqwestClient, encode_segment};
use dontyeet_primitives::address::Address;
use dontyeet_primitives::amount::Amount;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::BalanceFetcher;

/// ADA decimal places (1 ADA = 1,000,000 lovelace).
const ADA_DECIMALS: u8 = 6;

/// Blockfrost address info response (partial).
#[derive(Debug, Deserialize)]
struct AddressInfo {
    amount: Vec<AssetAmount>,
}

/// A single asset entry in the Blockfrost response.
#[derive(Debug, Deserialize)]
struct AssetAmount {
    unit: String,
    quantity: String,
}

/// Fetches Cardano balances via the Blockfrost REST API.
pub struct CardanoBalanceFetcher {
    endpoints: Endpoints,
    project_id: Option<String>,
}

impl CardanoBalanceFetcher {
    /// Create a balance fetcher from the configured API URLs and `project_id`.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>, project_id: Option<String>) -> Self {
        Self {
            endpoints: Endpoints::new(api_urls.clone()),
            project_id,
        }
    }
}

#[async_trait]
impl BalanceFetcher for CardanoBalanceFetcher {
    /// Fetch the on-chain ADA balance for an address.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the API call fails, or
    /// `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn fetch_balance(&self, address: &Address, network: &NetworkId) -> Result<Amount> {
        let base = self.endpoints.primary(network)?;

        let path = format!("/addresses/{}", encode_segment(address.as_str()));
        let addr_url = base
            .join(&path)
            .map_err(|e| DontYeetWalletError::Network(format!("URL join error: {e}")))?;

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let headers = crate::auth::project_id_headers(self.project_id.as_deref());
        let response = client
            .get_with_headers(&addr_url, &headers)
            .await
            .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

        let info: AddressInfo = response
            .json()
            .map_err(|e| DontYeetWalletError::Network(format!("address info parse error: {e}")))?;

        // Find the lovelace entry.
        let lovelace = info
            .amount
            .iter()
            .find(|a| a.unit == "lovelace")
            .ok_or_else(|| DontYeetWalletError::Chain("no lovelace balance in response".into()))?;

        let balance: u128 = lovelace
            .quantity
            .parse()
            .map_err(|e| DontYeetWalletError::Chain(format!("lovelace parse error: {e}")))?;

        Ok(Amount::from_raw(balance, ADA_DECIMALS))
    }
}

// Rust guideline compliant 2026-05-02
