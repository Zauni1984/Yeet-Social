//! EVM native balance fetching.
//!
//! Calls `eth_getBalance` via JSON-RPC to retrieve the native coin
//! balance in wei.

use std::collections::HashMap;

use async_trait::async_trait;
use url::Url;

use dontyeet_network::Endpoints;
use dontyeet_primitives::address::Address;
use dontyeet_primitives::amount::Amount;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::Result;
use dontyeet_primitives::traits::BalanceFetcher;

use crate::rpc;

/// EVM decimals for native coins (18 for all supported EVM chains).
const EVM_DECIMALS: u8 = 18;

/// Fetches native EVM balances via `eth_getBalance`.
pub struct EvmBalanceFetcher {
    endpoints: Endpoints,
}

impl EvmBalanceFetcher {
    /// Create a balance fetcher from the configured RPC URLs.
    #[must_use]
    pub fn new(rpc_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            endpoints: Endpoints::new(rpc_urls.clone()),
        }
    }
}

#[async_trait]
impl BalanceFetcher for EvmBalanceFetcher {
    /// Fetch the native coin balance for an EVM address.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the RPC call fails, or
    /// `DontYeetWalletError::NotFound` if no RPC URLs exist for the network.
    async fn fetch_balance(&self, address: &Address, network: &NetworkId) -> Result<Amount> {
        let urls = self.endpoints.all(network)?;

        let balance_hex: String = rpc::rpc_call(
            urls,
            "eth_getBalance",
            serde_json::json!([address.as_str(), "latest"]),
        )
        .await?;

        let wei = rpc::parse_hex_u128(&balance_hex)?;
        Ok(Amount::from_raw(wei, EVM_DECIMALS))
    }
}

// Rust guideline compliant 2026-05-02
