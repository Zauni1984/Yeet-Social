#![deny(clippy::all, clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]

//! `DontYeetWallet` Cardano chain implementation (CIP-1852, Blockfrost).
//!
//! Provides [`CardanoChainPlugin`] implementing the [`ChainPlugin`]
//! trait for Cardano. Uses Ed25519 keypairs, enterprise addresses
//! (blake2b-224), and the Blockfrost REST v0 API.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use dontyeet_chain_cardano::cardano_plugin;
//!
//! let plugin = cardano_plugin();
//! ```
//!
//! ## Supported Networks
//!
//! - **cardano-mainnet** — Cardano Mainnet
//! - **cardano-preprod** — Cardano Pre-Production Testnet
//! - **cardano-preview** — Cardano Preview Testnet
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
pub(crate) mod auth;
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
pub use config::CardanoConfig;
pub use error::{CardanoError, CardanoResult};
pub use keys::{CardanoAddressEncoder, CardanoKeyDeriver, derive_address};

// `rpc`-only re-exports.
#[cfg(feature = "rpc")]
pub use fees::CardanoFees;
#[cfg(feature = "rpc")]
pub use plugin::{CardanoChainPlugin, cardano_plugin};

// Rust guideline compliant 2026-05-02
