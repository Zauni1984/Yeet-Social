//! Per-wallet network selection.

use std::sync::Mutex;

use dontyeet_primitives::traits::NetworkProvider;
use dontyeet_primitives::{BlockchainNetwork, NetworkId};

use crate::error::{WalletError, WalletResult};

/// Thread-safe per-wallet network selection.
///
/// Initialized from the plugin's default network. Can be changed to any
/// network the plugin supports.
pub(crate) struct NetworkSelection {
    current: Mutex<NetworkId>,
}

impl NetworkSelection {
    /// Create from a network provider's default.
    pub(crate) fn new(provider: &dyn NetworkProvider) -> Self {
        Self {
            current: Mutex::new(provider.default_network().id.clone()),
        }
    }

    /// The current network ID.
    ///
    /// # Errors
    /// Returns `WalletError` if the mutex is poisoned.
    pub(crate) fn current_id(&self) -> WalletResult<NetworkId> {
        let guard = self
            .current
            .lock()
            .map_err(|e| WalletError::UnsupportedNetwork(format!("lock poisoned: {e}")))?;
        Ok(guard.clone())
    }

    /// Find the full network metadata for the current selection.
    ///
    /// # Errors
    /// Returns `WalletError::UnsupportedNetwork` if not found.
    pub(crate) fn current_network(
        &self,
        provider: &dyn NetworkProvider,
    ) -> WalletResult<BlockchainNetwork> {
        let id = self.current_id()?;
        provider
            .networks()
            .iter()
            .find(|n| n.id == id)
            .cloned()
            .ok_or_else(|| WalletError::UnsupportedNetwork(id.to_string()))
    }

    /// Switch to a different network.
    ///
    /// Validates that the network exists in the provider before switching.
    ///
    /// # Errors
    /// Returns `WalletError::UnsupportedNetwork` if the ID is not found.
    pub(crate) fn change(
        &self,
        network_id: &NetworkId,
        provider: &dyn NetworkProvider,
    ) -> WalletResult<()> {
        let exists = provider.networks().iter().any(|n| &n.id == network_id);
        if !exists {
            return Err(WalletError::UnsupportedNetwork(network_id.to_string()));
        }

        let mut guard = self
            .current
            .lock()
            .map_err(|e| WalletError::UnsupportedNetwork(format!("lock poisoned: {e}")))?;
        *guard = network_id.clone();
        Ok(())
    }
}

// Rust guideline compliant 2026-05-02
