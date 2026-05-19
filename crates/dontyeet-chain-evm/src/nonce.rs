//! Local nonce management for EVM transactions.
//!
//! Prevents race conditions when multiple sends are issued concurrently
//! by tracking the next nonce per address locally. The first send for a
//! given address fetches from the RPC node; subsequent sends increment
//! locally without a round-trip.

use std::collections::HashMap;

use tokio::sync::Mutex;
use url::Url;

use dontyeet_primitives::error::Result;

use crate::rpc;

/// Composite key: (network-qualified) address string.
type NonceKey = String;

/// Thread-safe local nonce tracker.
///
/// Holds a mutex-protected map of `address → next_nonce`. The mutex
/// is held across the RPC fetch on first use, which prevents two
/// concurrent calls from getting the same nonce.
pub struct NonceManager {
    nonces: Mutex<HashMap<NonceKey, u64>>,
}

impl NonceManager {
    /// Create an empty nonce manager.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nonces: Mutex::new(HashMap::new()),
        }
    }

    /// Get the next nonce for `address`, fetching from RPC if this is
    /// the first send for that address.
    ///
    /// The returned nonce is immediately incremented in the local map,
    /// so the next caller gets `nonce + 1` without an RPC call.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if the initial RPC fetch fails.
    pub async fn next_nonce(&self, address: &str, urls: &[Url]) -> Result<u64> {
        let mut map = self.nonces.lock().await;

        if let Some(nonce) = map.get_mut(address) {
            let current = *nonce;
            *nonce = current.wrapping_add(1);
            return Ok(current);
        }

        // First call for this address — fetch from RPC.
        let nonce_hex: String = rpc::rpc_call(
            urls,
            "eth_getTransactionCount",
            serde_json::json!([address, "pending"]),
        )
        .await?;
        let nonce = rpc::parse_hex_u64(&nonce_hex)?;

        // Store the *next* nonce so the next caller gets nonce + 1.
        map.insert(address.to_string(), nonce.wrapping_add(1));

        Ok(nonce)
    }

    /// Reset the tracked nonce for an address, forcing the next call
    /// to re-fetch from RPC.
    ///
    /// Call this after a transaction failure to re-sync with on-chain state.
    pub async fn reset(&self, address: &str) {
        self.nonces.lock().await.remove(address);
    }

    /// Clear all tracked nonces.
    pub async fn clear(&self) {
        self.nonces.lock().await.clear();
    }
}

impl Default for NonceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sequential_nonces_increment() {
        let mgr = NonceManager::new();
        // Pre-seed a nonce to avoid needing an RPC endpoint.
        mgr.nonces.lock().await.insert("0xABC".to_string(), 5);

        assert_eq!(mgr.next_nonce("0xABC", &[]).await.expect("seeded nonce"), 5);
        assert_eq!(mgr.next_nonce("0xABC", &[]).await.expect("seeded nonce"), 6);
        assert_eq!(mgr.next_nonce("0xABC", &[]).await.expect("seeded nonce"), 7);
    }

    #[tokio::test]
    async fn reset_clears_address() {
        let mgr = NonceManager::new();
        mgr.nonces.lock().await.insert("0xABC".to_string(), 10);

        assert_eq!(
            mgr.next_nonce("0xABC", &[]).await.expect("seeded nonce"),
            10
        );
        mgr.reset("0xABC").await;

        // After reset, the address is gone — next call would try RPC.
        assert!(mgr.nonces.lock().await.get("0xABC").is_none());
    }

    #[tokio::test]
    async fn separate_addresses_independent() {
        let mgr = NonceManager::new();
        {
            let mut map = mgr.nonces.lock().await;
            map.insert("0xAAA".to_string(), 0);
            map.insert("0xBBB".to_string(), 100);
        }

        assert_eq!(mgr.next_nonce("0xAAA", &[]).await.expect("seeded AAA"), 0);
        assert_eq!(mgr.next_nonce("0xBBB", &[]).await.expect("seeded BBB"), 100);
        assert_eq!(mgr.next_nonce("0xAAA", &[]).await.expect("seeded AAA"), 1);
        assert_eq!(mgr.next_nonce("0xBBB", &[]).await.expect("seeded BBB"), 101);
    }
}

// Rust guideline compliant 2026-05-02
