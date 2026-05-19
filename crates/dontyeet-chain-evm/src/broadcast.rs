//! EVM transaction broadcasting.
//!
//! Sends signed transactions to the network via `eth_sendRawTransaction`
//! and optionally polls for on-chain confirmation.

use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use url::Url;

use dontyeet_network::Endpoints;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::Result;
use dontyeet_primitives::traits::TransactionBroadcaster;
use dontyeet_primitives::transaction::{TxHash, TxStatus};

use crate::rpc;

/// Broadcasts signed EVM transactions via JSON-RPC.
pub struct EvmBroadcaster {
    endpoints: Endpoints,
}

impl EvmBroadcaster {
    /// Create a broadcaster from the configured RPC URLs.
    #[must_use]
    pub fn new(rpc_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            endpoints: Endpoints::new(rpc_urls.clone()),
        }
    }
}

#[async_trait]
impl TransactionBroadcaster for EvmBroadcaster {
    /// Broadcast a signed transaction and return the resulting tx hash.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the RPC call fails, or
    /// `DontYeetWalletError::NotFound` if no RPC URLs exist for the network.
    async fn broadcast(&self, signed_tx: &[u8], network: &NetworkId) -> Result<TxHash> {
        let urls = self.endpoints.all(network)?;

        let raw_hex = format!("0x{}", hex::encode(signed_tx));
        let tx_hash: String =
            rpc::rpc_call(urls, "eth_sendRawTransaction", serde_json::json!([raw_hex])).await?;

        Ok(TxHash::new(tx_hash))
    }
}

/// Default confirmation timeout.
const POLL_TIMEOUT: Duration = Duration::from_secs(120);
/// Initial poll interval.
const POLL_INITIAL_INTERVAL: Duration = Duration::from_secs(2);
/// Maximum poll interval (exponential backoff cap).
const POLL_MAX_INTERVAL: Duration = Duration::from_secs(16);

/// Minimal receipt fields we care about.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TxReceipt {
    /// Block number as hex string, present when mined.
    block_number: Option<String>,
    /// `"0x1"` = success, `"0x0"` = revert.
    status: Option<String>,
}

impl EvmBroadcaster {
    /// Poll `eth_getTransactionReceipt` until the transaction is confirmed,
    /// fails, or the timeout expires.
    ///
    /// Uses exponential backoff: 2s → 4s → 8s → 16s (capped).
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if every RPC poll call fails.
    pub async fn poll_receipt(&self, tx_hash: &TxHash, network: &NetworkId) -> Result<TxStatus> {
        let urls = self.endpoints.all(network)?;

        let deadline = tokio::time::Instant::now() + POLL_TIMEOUT;
        let mut interval = POLL_INITIAL_INTERVAL;

        loop {
            // Try fetching the receipt.
            let result: Option<TxReceipt> = rpc::rpc_call(
                urls,
                "eth_getTransactionReceipt",
                serde_json::json!([tx_hash.as_str()]),
            )
            .await?;

            if let Some(receipt) = result {
                // Parse block number.
                let block_number = receipt
                    .block_number
                    .as_deref()
                    .and_then(|h| rpc::parse_hex_u64(h).ok())
                    .unwrap_or(0);

                // Check status: "0x1" = success, "0x0" = revert.
                return if receipt.status.as_deref() == Some("0x0") {
                    Ok(TxStatus::Failed {
                        reason: "transaction reverted".into(),
                    })
                } else {
                    Ok(TxStatus::Confirmed { block_number })
                };
            }

            // Not yet mined — check timeout.
            if tokio::time::Instant::now() + interval > deadline {
                return Ok(TxStatus::Timeout);
            }

            tokio::time::sleep(interval).await;
            interval = (interval * 2).min(POLL_MAX_INTERVAL);
        }
    }
}

// Rust guideline compliant 2026-05-02
