//! EVM transaction building (with nonce) and signing.
//!
//! Server-side wrapper: ties together [`NonceManager`] (which talks
//! to the RPC node to allocate nonces) with the pure-crypto encode +
//! sign helpers in [`crate::signing`].
//!
//! Browser consumers go straight through [`crate::signing`] and
//! [`crate::wasm`] instead — they don't need this module's
//! [`NonceManager`] machinery.

use std::collections::HashMap;

use async_trait::async_trait;
use url::Url;

use dontyeet_primitives::address::Address;
use dontyeet_primitives::amount::Amount;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::KeyPair;
use dontyeet_primitives::traits::TransactionBuilder;

use crate::config::EvmChainConfig;
use crate::fees::EvmFees;
use crate::nonce::NonceManager;
use crate::signing;

/// Builds unsigned EVM legacy transactions.
///
/// Holds a local [`NonceManager`] to prevent concurrent sends from
/// allocating the same nonce. The first send for a given address
/// fetches the nonce from the RPC node; subsequent sends increment
/// locally.
pub struct EvmTransactionBuilder {
    evm_chain_id: u64,
    rpc_urls: HashMap<NetworkId, Vec<Url>>,
    nonce_manager: NonceManager,
}

impl EvmTransactionBuilder {
    /// Create a transaction builder from config.
    #[must_use]
    pub fn new(config: &EvmChainConfig) -> Self {
        Self {
            evm_chain_id: config.evm_chain_id_mainnet,
            rpc_urls: config.rpc_urls.clone(),
            nonce_manager: NonceManager::new(),
        }
    }

    /// Access the nonce manager (e.g. to reset after a failed transaction).
    #[must_use]
    pub fn nonce_manager(&self) -> &NonceManager {
        &self.nonce_manager
    }

    /// Get the EVM chain ID for EIP-155 signing.
    #[must_use]
    pub fn evm_chain_id(&self) -> u64 {
        self.evm_chain_id
    }
}

#[async_trait]
impl TransactionBuilder<EvmFees> for EvmTransactionBuilder {
    /// Build an unsigned RLP-encoded legacy transaction for a simple transfer.
    ///
    /// The returned bytes are the RLP encoding of
    /// `[nonce, gasPrice, gasLimit, to, value, data, chainId, 0, 0]`
    /// (EIP-155 unsigned format).
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Network` if nonce fetching fails, or
    /// `DontYeetWalletError::Validation` if `to` lacks a `0x` prefix or is
    /// not valid hex.
    async fn build_simple_transfer(
        &self,
        from: &KeyPair,
        to: &Address,
        amount: &Amount,
        fees: &EvmFees,
        network: &NetworkId,
    ) -> Result<Vec<u8>> {
        let urls = self
            .rpc_urls
            .get(network)
            .ok_or_else(|| DontYeetWalletError::NotFound(format!("no RPC URLs for {network}")))?;

        // Allocate nonce via the local tracker (prevents race conditions
        // when concurrent sends are in flight).
        let nonce = self
            .nonce_manager
            .next_nonce(from.address.as_str(), urls)
            .await?;

        // Parse destination address bytes.
        let to_hex = to
            .as_str()
            .strip_prefix("0x")
            .or_else(|| to.as_str().strip_prefix("0X"))
            .ok_or_else(|| DontYeetWalletError::Validation("address missing 0x prefix".into()))?;
        let to_bytes = hex::decode(to_hex).map_err(|e| DontYeetWalletError::Validation(e.to_string()))?;

        Ok(signing::rlp_encode_unsigned(
            nonce,
            fees.gas_price_wei,
            fees.gas_limit,
            &to_bytes,
            amount.raw(),
            &[],
            self.evm_chain_id,
        ))
    }
}

// EVM signing is provided by `dontyeet_chain::FnSigner` constructed
// inline in `crate::plugin::EvmChainPlugin::new`, which captures the
// EIP-155 chain id from config and delegates to
// `signing::sign_legacy_tx`. No dedicated signer struct lives here.

// Rust guideline compliant 2026-05-02
