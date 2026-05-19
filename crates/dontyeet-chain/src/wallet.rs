//! Chain-agnostic wallet orchestrator.
//!
//! The `Wallet` validates, preflights, delegates to a [`ChainPlugin`],
//! and formats results.  It never touches raw crypto directly.

use std::sync::Arc;

use dontyeet_primitives::traits::ChainPlugin;
use dontyeet_primitives::{
    Address, Amount, AssetInfo, BlockchainNetwork, ChainId, NetworkId, Seed,
};

use crate::error::{WalletError, WalletResult};
use crate::network::NetworkSelection;
use crate::send::SendResult;

/// A chain-agnostic wallet that delegates all chain-specific work to its plugin.
///
/// **Security:** The wallet does NOT hold a [`Seed`].  Operations that need
/// key material take `seed: &Seed` as a parameter.  The [`AccountManager`]
/// provides seeds on demand from the unlocked session.
pub struct Wallet {
    plugin: Arc<dyn ChainPlugin>,
    network: NetworkSelection,
}

impl Wallet {
    /// Create a new wallet wrapping the given chain plugin.
    ///
    /// The current network is initialized to the plugin's default.
    #[must_use]
    pub fn new(plugin: Arc<dyn ChainPlugin>) -> Self {
        let network = NetworkSelection::new(plugin.network_provider());
        Self { plugin, network }
    }

    /// Get an Arc reference to the underlying plugin.
    #[must_use]
    pub fn plugin_arc(&self) -> Arc<dyn ChainPlugin> {
        Arc::clone(&self.plugin)
    }

    /// Which chain this wallet operates on.
    #[must_use]
    pub fn chain_id(&self) -> &ChainId {
        self.plugin.chain_id()
    }

    /// Metadata for the chain's native asset (name, symbol, decimals).
    #[must_use]
    pub fn asset_info(&self) -> &AssetInfo {
        self.plugin.native_asset()
    }

    /// Derive the wallet address for the current network.
    ///
    /// # Errors
    /// Returns `WalletError` if key derivation fails.
    pub fn address(&self, seed: &Seed) -> WalletResult<Address> {
        let network_id = self.network.current_id()?;
        let keypair = self
            .plugin
            .key_deriver()
            .derive_keypair(seed, &network_id)?;
        Ok(keypair.address)
    }

    /// Fetch the native coin balance for the given address on the current network.
    ///
    /// # Errors
    /// Returns `WalletError` if balance fetching fails.
    pub async fn balance(&self, address: &Address) -> WalletResult<Amount> {
        let network_id = self.network.current_id()?;
        let balance = self
            .plugin
            .balance_fetcher()
            .fetch_balance(address, &network_id)
            .await?;
        Ok(balance)
    }

    /// Get a block explorer URL for this wallet's address.
    ///
    /// # Errors
    /// Returns `WalletError` if address derivation or URL formatting fails.
    pub fn explorer_url(&self, seed: &Seed) -> WalletResult<String> {
        let network_id = self.network.current_id()?;
        let keypair = self
            .plugin
            .key_deriver()
            .derive_keypair(seed, &network_id)?;
        let urls = self.plugin.network_provider().explorer_urls(&network_id)?;
        Ok(urls.format_address(keypair.address.as_str()))
    }

    /// The currently selected network.
    ///
    /// # Errors
    /// Returns `WalletError::UnsupportedNetwork` if the current ID is stale.
    pub fn current_network(&self) -> WalletResult<BlockchainNetwork> {
        self.network.current_network(self.plugin.network_provider())
    }

    /// All networks this chain supports.
    #[must_use]
    pub fn networks(&self) -> &[BlockchainNetwork] {
        self.plugin.network_provider().networks()
    }

    /// Switch to a different network.
    ///
    /// # Errors
    /// Returns `WalletError::UnsupportedNetwork` if the network ID is unknown.
    pub fn change_network(&self, network_id: &NetworkId) -> WalletResult<()> {
        self.network
            .change(network_id, self.plugin.network_provider())
    }

    /// Estimate fee tiers for the current network (type-erased).
    ///
    /// The returned `Box<dyn Any>` should be downcast to the chain's
    /// concrete `FeeTier<F>` type by the caller or passed directly to
    /// [`send`](Self::send).
    ///
    /// # Errors
    /// Returns `WalletError::FeeEstimation` if estimation fails.
    pub fn fee_estimator(&self) -> &dyn std::any::Any {
        self.plugin.fee_estimator_any()
    }

    /// Send native coin: validate → preflight → build → sign → broadcast.
    ///
    /// This is the core pipeline.  Each stage has a distinct error variant
    /// so callers know exactly where a failure occurred.
    ///
    /// # Arguments
    /// * `seed` — Master seed (borrowed, never stored).
    /// * `to` — Destination address.
    /// * `amount` — Amount to send in smallest unit.
    /// * `fees_any` — Chain-specific fee config (type-erased).
    ///
    /// # Errors
    /// Returns a stage-specific `WalletError` on failure.
    #[tracing::instrument(skip(self, seed, fees_any), fields(chain = %self.plugin.chain_id()))]
    pub async fn send(
        &self,
        seed: &Seed,
        to: &Address,
        amount: &Amount,
        fees_any: &dyn std::any::Any,
    ) -> WalletResult<SendResult> {
        let network_id = self.network.current_id()?;

        // Stage 1: VALIDATE destination address
        tracing::info!("stage 1: validating destination");
        self.plugin
            .address_encoder()
            .validate(to.as_str(), &network_id)
            .map_err(|e| WalletError::InvalidAddress(e.to_string()))?;

        // Stage 2: PREFLIGHT — derive keys, check balance
        tracing::info!("stage 2: preflight checks");
        let keypair = self
            .plugin
            .key_deriver()
            .derive_keypair(seed, &network_id)?;

        let private_key = keypair.private_key().ok_or(WalletError::NoPrivateKey)?;

        let balance = self
            .plugin
            .balance_fetcher()
            .fetch_balance(&keypair.address, &network_id)
            .await?;

        if balance < *amount {
            return Err(WalletError::InsufficientFunds {
                needed: amount.to_display_string(),
                available: balance.to_display_string(),
            });
        }

        // Stage 3+4: BUILD + SIGN + BROADCAST (delegated to plugin)
        tracing::info!("stage 3+4: building, signing, and broadcasting");

        let _ = (private_key, fees_any);
        let tx_hash = self
            .plugin
            .send_transfer(seed, to, amount, &network_id)
            .await
            .map_err(|e| WalletError::BroadcastFailed(e.to_string()))?;

        let explorer_urls = self.plugin.network_provider().explorer_urls(&network_id)?;

        let explorer_url = explorer_urls.format_tx(tx_hash.as_str());

        tracing::info!(%tx_hash, "transaction sent");
        Ok(SendResult {
            tx_hash,
            explorer_url,
        })
    }
}

impl std::fmt::Debug for Wallet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wallet")
            .field("chain_id", self.plugin.chain_id())
            .field("asset", &self.plugin.native_asset().symbol)
            .finish_non_exhaustive()
    }
}

// Rust guideline compliant 2026-05-02
