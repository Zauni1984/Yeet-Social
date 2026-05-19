#![deny(clippy::all, clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]

//! Kaspa chain plugin for `DontYeetWallet`.
//!
//! Provides a [`KaspaChainPlugin`] that handles the Kaspa `BlockDAG`
//! chain (GHOSTDAG protocol) with 1-second block times.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use dontyeet_chain_kaspa::kaspa_plugin;
//!
//! let plugin = kaspa_plugin();
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
pub mod history;
#[cfg(feature = "rpc")]
pub mod plugin;
#[cfg(feature = "rpc")]
mod rest;
#[cfg(feature = "rpc")]
pub mod transfer;

// Always-on re-exports.
pub use config::KaspaConfig;
pub use error::{KaspaError, KaspaResult};
pub use keys::{KaspaAddressEncoder, KaspaKeyDeriver, derive_address};

// `rpc`-only re-exports.
#[cfg(feature = "rpc")]
pub use fees::KaspaFees;
#[cfg(feature = "rpc")]
pub use plugin::{KaspaChainPlugin, kaspa_plugin};

// Rust guideline compliant 2026-05-02
