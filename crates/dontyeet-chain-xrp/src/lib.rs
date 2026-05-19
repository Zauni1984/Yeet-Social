#![deny(clippy::all, clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]

//! XRP Ledger chain plugin for `DontYeetWallet`.
//!
//! Provides [`XrpChainPlugin`] implementing the [`ChainPlugin`] trait for
//! the XRP Ledger. Uses secp256k1 keys, XRP's custom Base58 address
//! encoding, and the XRP Ledger JSON-RPC API for network operations.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use dontyeet_chain_xrp::xrp_plugin;
//!
//! let plugin = xrp_plugin();
//! ```
//!
//! ## Supported Networks
//!
//! - **xrp-mainnet** — XRP Mainnet
//! - **xrp-testnet** — XRP Testnet
//!
//! [`ChainPlugin`]: dontyeet_primitives::traits::ChainPlugin

// Always-on modules (pure crypto, WASM-compatible).
pub mod config;
pub mod error;
pub mod keys;
pub mod signing;

// Browser-only: in-browser balance reads via `gloo_net`. Compiles
// only on `wasm32` targets.
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
pub use config::XrpConfig;
pub use error::{XrpError, XrpResult};
pub use keys::{XrpAddressEncoder, XrpKeyDeriver, derive_address};

// `rpc`-only re-exports.
#[cfg(feature = "rpc")]
pub use fees::XrpFees;
#[cfg(feature = "rpc")]
pub use plugin::{XrpChainPlugin, xrp_plugin};

// Rust guideline compliant 2026-05-02
