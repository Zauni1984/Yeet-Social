//! Plugin bundle traits.
//!
//! A `ChainPlugin` bundles all per-chain capabilities into a single
//! registrable object.  An `IdentityPlugin` does the same for identity
//! providers.  Integration crates implement these; the registry stores them.

use async_trait::async_trait;

use crate::asset::AssetInfo;
use crate::chain::ChainId;
use crate::chain::NetworkId;
use crate::secret::Seed;
use crate::traits::chain::{
    AddressEncoder, BalanceFetcher, KeyDeriver, NetworkProvider, RpcEndpointProvider,
    TransactionBroadcaster, TransactionSigner,
};
use crate::traits::identity::IdentityResolver;
use crate::transaction::{RpcHealthResult, StandardizedFeeTier, TxHash, TxHistoryItem};
use crate::{Address, Amount, DontYeetWalletError, Result};

/// A complete chain integration bundled behind a single trait object.
///
/// Each supported chain implements this once.  The registry holds
/// `Box<dyn ChainPlugin>` and dispatches by [`ChainId`].
#[async_trait]
pub trait ChainPlugin: Send + Sync {
    /// Which chain this plugin handles.
    fn chain_id(&self) -> &ChainId;

    /// Metadata for the chain's native asset.
    fn native_asset(&self) -> &AssetInfo;

    /// Key derivation capability.
    fn key_deriver(&self) -> &dyn KeyDeriver;

    /// Network metadata.
    fn network_provider(&self) -> &dyn NetworkProvider;

    /// RPC endpoint URLs.
    fn rpc_endpoints(&self) -> &dyn RpcEndpointProvider;

    /// Address validation for this chain.
    fn address_encoder(&self) -> &dyn AddressEncoder;

    /// Native balance fetching.
    fn balance_fetcher(&self) -> &dyn BalanceFetcher;

    /// Fee estimation.
    ///
    /// Returns a type-erased estimator.  The concrete fee type is
    /// chain-specific and handled internally by the plugin.
    fn fee_estimator_any(&self) -> &dyn std::any::Any;

    /// Transaction signing.
    fn signer(&self) -> &dyn TransactionSigner;

    /// Transaction building (type-erased — fee type is chain-specific).
    ///
    /// Integration crates downcast this to their concrete builder type.
    fn tx_builder_any(&self) -> &dyn std::any::Any;

    /// Transaction broadcasting.
    fn broadcaster(&self) -> &dyn TransactionBroadcaster;

    /// Return chain-agnostic fee estimates for display in the UI.
    ///
    /// Each chain overrides this to call its own fee estimator and format
    /// the result into [`StandardizedFeeTier`] with human-readable labels.
    async fn estimate_fee_display(&self, network: &NetworkId) -> Result<StandardizedFeeTier> {
        let _ = network;
        Err(DontYeetWalletError::Unsupported(format!(
            "fee estimation not implemented for {}",
            self.chain_id()
        )))
    }

    /// Execute a full send pipeline: estimate fees → build → sign → broadcast.
    ///
    /// This default implementation returns `Unsupported`. Chain plugins that
    /// support sending override this with their concrete implementation.
    async fn send_transfer(
        &self,
        seed: &Seed,
        to: &Address,
        amount: &Amount,
        network: &NetworkId,
    ) -> Result<TxHash> {
        let _ = (seed, to, amount, network);
        Err(DontYeetWalletError::Unsupported(format!(
            "send not implemented for {}",
            self.chain_id()
        )))
    }

    /// Fetch transaction history for the given address.
    ///
    /// Override in chain plugins that have indexer API integrations.
    async fn fetch_history(
        &self,
        address: &Address,
        network: &NetworkId,
        limit: usize,
    ) -> Result<Vec<TxHistoryItem>> {
        let _ = (address, network, limit);
        Err(DontYeetWalletError::Unsupported(format!(
            "transaction history not implemented for {}",
            self.chain_id()
        )))
    }

    /// Fetch the balance of a specific token by contract address.
    ///
    /// Returns `(amount, symbol)`. Override in chain plugins that support tokens.
    async fn fetch_token_balance(
        &self,
        address: &Address,
        contract: &str,
        network: &NetworkId,
    ) -> Result<(Amount, String)> {
        let _ = (address, contract, network);
        Err(DontYeetWalletError::Unsupported(format!(
            "token balance not implemented for {}",
            self.chain_id()
        )))
    }

    /// Ping the chain's RPC endpoint and return health info.
    ///
    /// Override in chain plugins to provide per-chain health checks.
    async fn check_rpc_health(&self, network: &NetworkId) -> Result<RpcHealthResult> {
        let _ = network;
        Err(DontYeetWalletError::Unsupported(format!(
            "RPC health check not implemented for {}",
            self.chain_id()
        )))
    }
}

/// A complete identity provider bundled behind a single trait object.
pub trait IdentityPlugin: Send + Sync {
    /// Human-readable provider name, e.g. `"email"`, `"signal"`.
    fn provider_name(&self) -> &str;

    /// The resolver for this provider.
    fn resolver(&self) -> &dyn IdentityResolver;

    /// Example identifier patterns this provider handles (for UI hints).
    fn example_patterns(&self) -> &[&str];
}

// Rust guideline compliant 2026-05-02
