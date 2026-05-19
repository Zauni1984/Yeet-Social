#![deny(clippy::all, clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]

//! Chain-agnostic wallet orchestration for `DontYeetWallet`.
//!
//! This is the **Services** layer.  The [`Wallet`] validates, preflights,
//! and delegates chain-specific work to a
//! [`ChainPlugin`](dontyeet_primitives::traits::ChainPlugin).
//!
//! ## Send Pipeline
//!
//! ```text
//! Stage 1: VALIDATE   → address format check
//! Stage 2: PREFLIGHT  → derive keys, check balance
//! Stage 3: BUILD+SIGN → build unsigned tx, sign with private key
//! Stage 4: BROADCAST  → submit to network, return explorer link
//! ```

pub mod error;
pub mod fn_signer;
pub mod macros;
pub mod send;
pub mod wallet;

mod network;

pub use error::{WalletError, WalletResult};
pub use fn_signer::FnSigner;
pub use send::SendResult;
pub use wallet::Wallet;

// Rust guideline compliant 2026-05-02
