#![deny(clippy::all, clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]

//! Bitcoin chain plugin for `DontYeetWallet`.
//!
//! Provides [`BtcChainPlugin`] implementing the [`ChainPlugin`] trait for
//! Bitcoin. Uses the UTXO model, P2WPKH (segwit) addresses, and the
//! Mempool.space REST API for network operations.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use dontyeet_chain_bitcoin::bitcoin_plugin;
//!
//! let plugin = bitcoin_plugin();
//! ```
//!
//! ## Supported Networks
//!
//! - **bitcoin-mainnet** — Bitcoin Mainnet
//! - **bitcoin-testnet4** — Bitcoin Testnet4
//! - **bitcoin-signet** — Bitcoin Signet
//!
//! [`ChainPlugin`]: dontyeet_primitives::traits::ChainPlugin

// Always-on modules (pure crypto, WASM-compatible).
pub mod config;
pub mod error;
pub mod keys;
pub mod signing;

// Browser-only: in-browser balance reads via `gloo_net`. Compiles
// only on `wasm32` targets, so server builds (with `feature = "rpc"`)
// stay completely unaffected.
#[cfg(target_arch = "wasm32")]
pub mod wasm;

// `rpc`-only modules.
#[cfg(feature = "rpc")]
pub mod balance;
#[cfg(feature = "rpc")]
pub mod broadcast;
#[cfg(feature = "rpc")]
pub mod fees;
#[cfg(feature = "rpc")]
pub mod history;
#[cfg(feature = "rpc")]
pub mod plugin;
#[cfg(feature = "rpc")]
pub mod transfer;

// Always-on re-exports.
pub use config::{AddressFormat, BtcConfig};
pub use error::{BtcError, BtcResult};
pub use keys::{BtcAddressEncoder, BtcKeyDeriver, derive_address};

// `rpc`-only re-exports.
#[cfg(feature = "rpc")]
pub use fees::BtcFees;
#[cfg(feature = "rpc")]
pub use plugin::{BtcChainPlugin, bitcoin_plugin};

// Rust guideline compliant 2026-05-02
