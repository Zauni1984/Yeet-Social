//! Per-chain capability traits.

use async_trait::async_trait;
use url::Url;

use crate::address::Address;
use crate::amount::Amount;
use crate::chain::NetworkId;
use crate::error::Result;
use crate::network::{BlockchainNetwork, ExplorerUrls};
use crate::secret::{KeyPair, PrivateKey, Seed};
use crate::transaction::TxHash;

/// Derive a keypair from a seed for a specific network.
pub trait KeyDeriver: Send + Sync {
    /// Derive a [`KeyPair`] from the master seed for the given network.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError` if key derivation fails.
    fn derive_keypair(&self, seed: &Seed, network: &NetworkId) -> Result<KeyPair>;
}

/// Encode a public key into a chain-specific address.
pub trait AddressEncoder: Send + Sync {
    /// Encode raw public key bytes into a human-readable address.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError` if encoding fails.
    fn encode(&self, public_key: &[u8], network: &NetworkId) -> Result<Address>;

    /// Validate an address string for this chain.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError` if the address is invalid.
    fn validate(&self, address: &str, network: &NetworkId) -> Result<()>;
}

/// Sign a raw transaction.
pub trait TransactionSigner: Send + Sync {
    /// Sign `unsigned_tx` bytes with the given private key.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError` if signing fails.
    fn sign(&self, unsigned_tx: &[u8], private_key: &PrivateKey) -> Result<Vec<u8>>;
}

/// Build an unsigned transaction for a simple transfer.
///
/// Generic over `F` (the chain's fee type).
#[async_trait]
pub trait TransactionBuilder<F>: Send + Sync {
    /// Build an unsigned simple transfer transaction.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError` if the transaction cannot be built.
    async fn build_simple_transfer(
        &self,
        from: &KeyPair,
        to: &Address,
        amount: &Amount,
        fees: &F,
        network: &NetworkId,
    ) -> Result<Vec<u8>>;
}

/// Fetch the native balance for an address.
#[async_trait]
pub trait BalanceFetcher: Send + Sync {
    /// Fetch the current native coin balance.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError` if the balance cannot be fetched.
    async fn fetch_balance(&self, address: &Address, network: &NetworkId) -> Result<Amount>;
}

/// Fetch a token balance for an address.
///
/// Generic over `H` — the chain-specific token handle (e.g. contract address,
/// mint address, policy ID).
#[async_trait]
pub trait TokenBalanceFetcher<H>: Send + Sync {
    /// Fetch the token balance for the given handle.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError` if the token balance cannot be fetched.
    async fn fetch_token_balance(
        &self,
        address: &Address,
        handle: &H,
        network: &NetworkId,
    ) -> Result<Amount>;
}

/// Estimate fee tiers for a network.
#[async_trait]
pub trait FeeEstimator<F>: Send + Sync {
    /// Return slow / standard / fast fee suggestions.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError` if fee estimation fails.
    async fn estimate_fees(&self, network: &NetworkId) -> Result<crate::transaction::FeeTier<F>>;
}

/// Broadcast a signed transaction.
#[async_trait]
pub trait TransactionBroadcaster: Send + Sync {
    /// Submit signed transaction bytes and return the tx hash.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError` if broadcasting fails.
    async fn broadcast(&self, signed_tx: &[u8], network: &NetworkId) -> Result<TxHash>;
}

/// Fetch transaction history from a chain indexer or explorer API.
#[async_trait]
pub trait TransactionHistoryFetcher: Send + Sync {
    /// Fetch the most recent transactions for an address.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError` if the indexer API is unreachable or unsupported.
    async fn fetch_history(
        &self,
        address: &Address,
        network: &NetworkId,
        limit: usize,
    ) -> Result<Vec<crate::transaction::TxHistoryItem>>;
}

/// Provide network metadata for a chain.
pub trait NetworkProvider: Send + Sync {
    /// All networks this chain supports.
    fn networks(&self) -> &[BlockchainNetwork];

    /// The default network (usually mainnet).
    fn default_network(&self) -> &BlockchainNetwork;

    /// Explorer URLs for a given network.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError` if the network is not supported.
    fn explorer_urls(&self, network: &NetworkId) -> Result<ExplorerUrls>;
}

/// Provide RPC endpoint URLs for a network.
pub trait RpcEndpointProvider: Send + Sync {
    /// Ordered list of RPC URLs for the given network (first = preferred).
    ///
    /// # Errors
    /// Returns `DontYeetWalletError` if the network is not supported.
    fn rpc_urls(&self, network: &NetworkId) -> Result<Vec<Url>>;
}

// Rust guideline compliant 2026-05-02
