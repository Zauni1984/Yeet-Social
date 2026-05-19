#![deny(clippy::all, clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]

//! Solana chain plugin for `DontYeetWallet`.
//!
//! Provides [`SolChainPlugin`] implementing the [`ChainPlugin`] trait for
//! Solana. Uses Ed25519 keys, Base58 addresses, and the Solana JSON-RPC
//! API for network operations.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use dontyeet_chain_solana::solana_plugin;
//!
//! let plugin = solana_plugin();
//! ```
//!
//! ## Supported Networks
//!
//! - **solana-mainnet** — Solana Mainnet Beta
//! - **solana-devnet** — Solana Devnet
//! - **solana-testnet** — Solana Testnet
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
pub mod token_balance;
#[cfg(feature = "rpc")]
pub mod transfer;

// Always-on re-exports.
pub use config::SolConfig;
pub use error::{SolError, SolResult};
pub use keys::{SolAddressEncoder, SolKeyDeriver, derive_address};

// `rpc`-only re-exports.
#[cfg(feature = "rpc")]
pub use fees::SolanaFees;
#[cfg(feature = "rpc")]
pub use plugin::{SolChainPlugin, solana_plugin};

// Rust guideline compliant 2026-05-02
