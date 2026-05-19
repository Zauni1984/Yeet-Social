#![deny(clippy::all, clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]

//! TRON chain plugin for `DontYeetWallet`.
//!
//! Provides [`TronChainPlugin`] implementing the [`ChainPlugin`] trait for
//! TRON. Uses secp256k1 key derivation (BIP-44, coin type 195),
//! `Base58Check` address encoding (version byte `0x41`, `T` prefix), and
//! the `TronGrid` REST API for network operations.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use dontyeet_chain_tron::tron_plugin;
//!
//! let plugin = tron_plugin();
//! ```
//!
//! ## Supported Networks
//!
//! - **tron-mainnet** -- TRON Mainnet
//! - **tron-shasta** -- TRON Shasta Testnet
//! - **tron-nile** -- TRON Nile Testnet
//!
//! [`ChainPlugin`]: dontyeet_primitives::traits::ChainPlugin

// Always-on modules (pure crypto, WASM-compatible).
pub mod config;
pub mod error;
pub mod keys;
pub mod signing;

// Browser-only: in-browser balance reads via `gloo_net`. Compiles
// only on `wasm32` targets so server builds (with `feature = "rpc"`)
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
pub use config::TronConfig;
pub use error::{TronError, TronResult};
pub use keys::{TronAddressEncoder, TronKeyDeriver, derive_address};

// `rpc`-only re-exports.
#[cfg(feature = "rpc")]
pub use fees::TronFees;
#[cfg(feature = "rpc")]
pub use plugin::{TronChainPlugin, tron_plugin};

// Rust guideline compliant 2026-05-02
