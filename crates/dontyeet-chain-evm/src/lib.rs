#![deny(clippy::all, clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]

//! EVM chain plugin for `DontYeetWallet`.
//!
//! Provides a single [`EvmChainPlugin`] that handles **all** EVM-compatible
//! chains (Ethereum, Polygon, BNB, Avalanche, Sonic) via parameterization.
//! Each chain is constructed through a factory function in [`chains`].
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use dontyeet_chain_evm::chains::ethereum_plugin;
//!
//! let plugin = ethereum_plugin();
//! ```

// Always-on modules (pure crypto, WASM-compatible).
pub mod config;
pub mod error;
pub mod keys;
pub mod signing;

// Browser-only: in-browser balance reads + transaction signing via
// `gloo_net`. Compiles only on `wasm32` targets so server builds
// (with `feature = "rpc"`) stay completely unaffected.
#[cfg(target_arch = "wasm32")]
pub mod wasm;

// `rpc`-only modules: RPC clients, fee estimation, transaction
// broadcast, etc. Gated so wasm32 consumers can exclude them.
#[cfg(feature = "rpc")]
pub mod balance;
#[cfg(feature = "rpc")]
pub mod broadcast;
#[cfg(feature = "rpc")]
pub mod chains;
#[cfg(feature = "rpc")]
pub mod fees;
#[cfg(feature = "rpc")]
pub mod history;
#[cfg(feature = "rpc")]
pub mod nonce;
#[cfg(feature = "rpc")]
pub mod plugin;
#[cfg(feature = "rpc")]
mod rpc;
#[cfg(feature = "rpc")]
pub mod token_balance;
#[cfg(feature = "rpc")]
pub mod tx;

// Always-on re-exports.
pub use config::EvmChainConfig;
pub use error::{EvmError, EvmResult};
pub use keys::{EvmAddressEncoder, EvmKeyDeriver, derive_address};

// `rpc`-only re-exports.
#[cfg(feature = "rpc")]
pub use chains::{avalanche_plugin, bnb_plugin, ethereum_plugin, polygon_plugin, sonic_plugin};
#[cfg(feature = "rpc")]
pub use fees::EvmFees;
#[cfg(feature = "rpc")]
pub use plugin::EvmChainPlugin;

// Rust guideline compliant 2026-05-02
