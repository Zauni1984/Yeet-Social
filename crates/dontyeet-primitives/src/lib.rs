#![deny(clippy::all, clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]

//! Shared types, traits, and error definitions for `DontYeetWallet`.
//!
//! This is the **Foundation** layer — every other crate in the workspace
//! depends on it.  It has zero internal dependencies.

pub mod address;
pub mod amount;
pub mod asset;
pub mod chain;
pub mod error;
pub mod network;
pub mod secret;
pub mod traits;
pub mod transaction;

// Re-export the most commonly used items at crate root for convenience.
pub use address::Address;
pub use amount::{Amount, FiatAmount};
pub use asset::{AssetInfo, AssetKind};
pub use chain::{ChainId, NetworkCategory, NetworkId};
pub use error::{DontYeetWalletError, Result};
pub use network::{BlockchainNetwork, ExplorerUrls};
pub use secret::{KeyPair, Mnemonic, PrivateKey, Seed};
pub use transaction::{
    FeeTier, RpcHealthResult, SimpleTxParams, StandardizedFee, StandardizedFeeTier, TxConfirmation,
    TxDirection, TxHash, TxHistoryItem, TxStatus,
};

// Rust guideline compliant 2026-05-02
