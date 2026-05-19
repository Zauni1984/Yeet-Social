#![deny(clippy::all, clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]

//! Kadena community chain plugin for `DontYeetWallet`.
//!
//! Provides a [`KadenaChainPlugin`] that handles the Kadena Chainweb
//! chain (community edition, forked Nov 2025) with 20 parallel `PoW` chains.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use dontyeet_chain_kadena::kadena_plugin;
//!
//! let plugin = kadena_plugin();
//! ```

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
pub mod plugin;
#[cfg(feature = "rpc")]
mod rest;

// Always-on re-exports.
pub use config::KadenaConfig;
pub use error::{KadenaError, KadenaResult};
pub use keys::{KadenaAddressEncoder, KadenaKeyDeriver, derive_address};

// `rpc`-only re-exports.
#[cfg(feature = "rpc")]
pub use fees::KadenaFees;
#[cfg(feature = "rpc")]
pub use plugin::{KadenaChainPlugin, kadena_plugin};

// Rust guideline compliant 2026-05-02
