#![deny(clippy::all, clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]

//! Account lifecycle management for `DontYeetWallet`.
//!
//! This is the **Services** layer — depends on primitives, crypto, storage.
//!
//! ## State Machine
//!
//! ```text
//! [NoAccount] --create()--> [Locked] --login()--> [Unlocked] --logout()--> [Locked]
//! ```
//!
//! ## Modules
//!
//! - [`manager`] — `AccountManager`: create, login, logout, change password, delete
//! - [`session`] — Session state machine (encryption key gating)
//! - [`mnemonic_repo`] — Login-gated mnemonic CRUD

pub mod error;
pub mod manager;
pub mod mnemonic_repo;
pub mod session;

pub use error::{AccountError, AccountResult};
pub use manager::AccountManager;
pub use session::Session;

// Rust guideline compliant 2026-05-02
