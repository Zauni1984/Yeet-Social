//! Trait definitions for the `DontYeetWallet` plugin architecture.
//!
//! These traits form the contracts between layers.  Integration-layer crates
//! implement them; service-layer crates consume them.

mod chain;
mod identity;
mod plugin;

pub use chain::{
    AddressEncoder, BalanceFetcher, FeeEstimator, KeyDeriver, NetworkProvider, RpcEndpointProvider,
    TokenBalanceFetcher, TransactionBroadcaster, TransactionBuilder, TransactionHistoryFetcher,
    TransactionSigner,
};
pub use identity::{IdentityRecord, IdentityResolver};
pub use plugin::{ChainPlugin, IdentityPlugin};

// Rust guideline compliant 2026-05-02
