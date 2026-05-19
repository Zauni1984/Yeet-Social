#![deny(clippy::all, clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]

//! `DontYeetWallet` Algorand chain implementation (Ed25519, ASA tokens).
//!
//! Provides [`AlgoChainPlugin`] implementing the [`ChainPlugin`] trait for
//! Algorand. Uses Ed25519 keypairs, Base32-encoded addresses with
//! SHA-512/256 checksums, and the Algod REST v2 API.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use dontyeet_chain_algorand::algorand_plugin;
//!
//! let plugin = algorand_plugin();
//! ```
//!
//! ## Supported Networks
//!
//! - **algorand-mainnet** — Algorand Mainnet (Nodely)
//! - **algorand-testnet** — Algorand Testnet
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
pub use config::AlgoConfig;
pub use error::{AlgoError, AlgoResult};
pub use keys::{AlgoAddressEncoder, AlgoKeyDeriver, derive_address};

// `rpc`-only re-exports.
#[cfg(feature = "rpc")]
pub use fees::AlgoFees;
#[cfg(feature = "rpc")]
pub use plugin::{AlgoChainPlugin, algorand_plugin};

// Rust guideline compliant 2026-05-02
