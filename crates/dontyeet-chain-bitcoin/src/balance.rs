//! Bitcoin balance fetching via Mempool.space REST API.
//!
//! Queries `GET /address/{addr}` and computes:
//! `balance = chain_stats.funded_txo_sum - chain_stats.spent_txo_sum`

use std::collections::HashMap;

use async_trait::async_trait;
use url::Url;

use dontyeet_network::{Endpoints, HttpClient, ReqwestClient, encode_segment};
use dontyeet_primitives::address::Address;
use dontyeet_primitives::amount::Amount;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::BalanceFetcher;

/// BTC decimal places (1 BTC = `100_000_000` satoshis).
const BTC_DECIMALS: u8 = 8;

/// Mempool.space address stats response (partial).
#[derive(Debug, serde::Deserialize)]
struct AddressInfo {
    chain_stats: ChainStats,
}

/// On-chain statistics for an address.
#[derive(Debug, serde::Deserialize)]
struct ChainStats {
    funded_txo_sum: u64,
    spent_txo_sum: u64,
}

/// Fetches Bitcoin balances via the Mempool.space REST API.
pub struct BtcBalanceFetcher {
    endpoints: Endpoints,
}

impl BtcBalanceFetcher {
    /// Create a balance fetcher from the configured API URLs.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            endpoints: Endpoints::new(api_urls.clone()),
        }
    }
}

#[async_trait]
impl BalanceFetcher for BtcBalanceFetcher {
    /// Fetch the on-chain BTC balance for an address (in satoshis).
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the API call fails, or
    /// `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn fetch_balance(&self, address: &Address, network: &NetworkId) -> Result<Amount> {
        let base = self.endpoints.primary(network)?;

        let path = format!("/address/{}", encode_segment(address.as_str()));
        let addr_url = base
            .join(&path)
            .map_err(|e| DontYeetWalletError::Network(format!("URL join error: {e}")))?;

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
        let response = client
            .get(&addr_url)
            .await
            .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

        let info: AddressInfo = response
            .json()
            .map_err(|e| DontYeetWalletError::Network(format!("address info parse error: {e}")))?;

        let balance_sats = info
            .chain_stats
            .funded_txo_sum
            .checked_sub(info.chain_stats.spent_txo_sum)
            .ok_or_else(|| DontYeetWalletError::Chain("balance underflow: spent > funded".into()))?;

        Ok(Amount::from_raw(u128::from(balance_sats), BTC_DECIMALS))
    }
}

// Rust guideline compliant 2026-05-02
