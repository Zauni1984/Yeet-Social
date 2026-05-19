//! Kadena native balance fetching.
//!
//! Queries the Kadena Chainweb Pact API to retrieve the KDA balance
//! using the `coin.get-balance` Pact function on chain 0.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::Deserialize;
use url::Url;

use dontyeet_network::Endpoints;
use dontyeet_primitives::address::Address;
use dontyeet_primitives::amount::Amount;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::BalanceFetcher;

use crate::rest;

/// KDA has 12 decimal places (Pact coin contract precision).
const KDA_DECIMALS: u8 = 12;

/// Default chain to query for balance (chain 0).
const DEFAULT_CHAIN: &str = "0";

/// JSON response from the Pact `/local` endpoint.
#[derive(Debug, Deserialize)]
struct PactLocalResponse {
    /// The result object from Pact execution.
    result: PactResult,
}

/// The `result` field inside a Pact local response.
#[derive(Debug, Deserialize)]
struct PactResult {
    /// Status: `"success"` or `"failure"`.
    status: String,
    /// The returned data (balance as a decimal number for coin.get-balance).
    data: serde_json::Value,
}

/// Fetches native KDA balances via the Kadena Pact API.
pub struct KadenaBalanceFetcher {
    endpoints: Endpoints,
    network_versions: HashMap<NetworkId, String>,
}

impl KadenaBalanceFetcher {
    /// Create a balance fetcher from the configured API URLs and network versions.
    #[must_use]
    pub fn new(
        api_urls: &HashMap<NetworkId, Vec<Url>>,
        network_versions: &HashMap<NetworkId, String>,
    ) -> Self {
        Self {
            endpoints: Endpoints::new(api_urls.clone()),
            network_versions: network_versions.clone(),
        }
    }
}

#[async_trait]
impl BalanceFetcher for KadenaBalanceFetcher {
    /// Fetch the native KDA balance for an address.
    ///
    /// Calls the Pact `/local` endpoint on chain 0 with
    /// `(coin.get-balance "<account>")` and parses the decimal result.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the API call fails, or
    /// `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn fetch_balance(&self, address: &Address, network: &NetworkId) -> Result<Amount> {
        let urls = self.endpoints.all(network)?;

        let version = self
            .network_versions
            .get(network)
            .ok_or_else(|| DontYeetWalletError::NotFound(format!("no network version for {network}")))?;

        let path = format!("chainweb/0.0/{version}/chain/{DEFAULT_CHAIN}/pact/api/v1/local");

        let account = address.as_str();
        let cmd_body = serde_json::json!({
            "cmd": serde_json::json!({
                "networkId": version,
                "payload": {
                    "exec": {
                        "data": {},
                        "code": format!("(coin.get-balance \"{account}\")")
                    }
                },
                "signers": [],
                "meta": {
                    "chainId": DEFAULT_CHAIN,
                    "sender": "",
                    "gasLimit": 1000,
                    "gasPrice": 1e-8_f64,
                    "ttl": 600,
                    "creationTime": 0
                },
                "nonce": "balance-query"
            }).to_string(),
            "hash": "",
            "sigs": []
        });

        let resp: PactLocalResponse = rest::rest_post(urls, &path, &cmd_body).await?;

        if resp.result.status != "success" {
            return Ok(Amount::from_raw(0, KDA_DECIMALS));
        }

        parse_kda_balance(&resp.result.data)
    }
}

/// Parse a KDA balance from a Pact result value.
///
/// Pact returns balances as decimal numbers (e.g. `1.5` or `{"decimal": "1.5"}`).
fn parse_kda_balance(data: &serde_json::Value) -> Result<Amount> {
    let decimal_str = match data {
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Object(obj) => obj
            .get("decimal")
            .and_then(|v| v.as_str())
            .unwrap_or("0")
            .to_string(),
        serde_json::Value::String(s) => s.clone(),
        _ => return Ok(Amount::from_raw(0, KDA_DECIMALS)),
    };

    // Parse the decimal string and convert to raw units (12 decimal places).
    let parts: Vec<&str> = decimal_str.split('.').collect();
    let integer_part: u128 = parts[0]
        .parse()
        .map_err(|e| DontYeetWalletError::Chain(format!("invalid balance integer: {e}")))?;

    let fractional_raw = if parts.len() > 1 {
        let frac = parts[1];
        let padded = format!("{frac:0<width$}", width = KDA_DECIMALS as usize);
        let truncated = &padded[..KDA_DECIMALS as usize];
        truncated
            .parse::<u128>()
            .map_err(|e| DontYeetWalletError::Chain(format!("invalid balance fraction: {e}")))?
    } else {
        0
    };

    let multiplier: u128 = 10u128.pow(u32::from(KDA_DECIMALS));
    let raw = integer_part
        .checked_mul(multiplier)
        .and_then(|v| v.checked_add(fractional_raw))
        .ok_or_else(|| DontYeetWalletError::Chain("balance arithmetic overflow".into()))?;

    Ok(Amount::from_raw(raw, KDA_DECIMALS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_number_balance() {
        let data = serde_json::json!(1.5);
        let amount = parse_kda_balance(&data).expect("parse");
        assert_eq!(amount.raw(), 1_500_000_000_000); // 1.5 * 10^12
    }

    #[test]
    fn parse_decimal_object_balance() {
        let data = serde_json::json!({"decimal": "42.123456789012"});
        let amount = parse_kda_balance(&data).expect("parse");
        assert_eq!(amount.raw(), 42_123_456_789_012);
    }

    #[test]
    fn parse_zero_balance() {
        let data = serde_json::json!(0);
        let amount = parse_kda_balance(&data).expect("parse");
        assert_eq!(amount.raw(), 0);
    }

    #[test]
    fn parse_integer_only() {
        let data = serde_json::json!(100);
        let amount = parse_kda_balance(&data).expect("parse");
        assert_eq!(amount.raw(), 100_000_000_000_000); // 100 * 10^12
    }
}

// Rust guideline compliant 2026-05-02
