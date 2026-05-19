#![deny(clippy::all, clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]

//! HTTP client, JSON-RPC, caching, rate limiting, and privacy layer for
//! `DontYeetWallet`.
//!
//! This is the **Services** layer — depends on `dontyeet-primitives`.
//!
//! ## Modules
//!
//! - [`client`] — `HttpClient` trait + `ReqwestClient` (optional Tor/SOCKS5)
//! - [`jsonrpc`] — JSON-RPC 2.0 `RpcClient`
//! - [`retry`] — Exponential backoff for transient failures
//! - [`rate_limit`] — Token-bucket rate limiter
//! - [`cache`] — TTL-based response cache
//! - [`privacy`] — Tor toggle + endpoint rotation
//! - [`endpoints`] — Per-network URL bag with primary-first lookup
//! - [`network_catalog`] — Catalog of supported networks + endpoint URLs
//!   (implements `NetworkProvider` / `RpcEndpointProvider`)

pub mod cache;
pub mod client;
pub mod endpoints;
pub mod error;
pub mod jsonrpc;
pub mod network_catalog;
pub mod path;
pub mod privacy;
pub mod rate_limit;
pub mod retry;

pub use cache::TtlCache;
pub use client::{HttpClient, ReqwestClient, Response};
pub use endpoints::Endpoints;
pub use error::{NetworkError, NetworkResult};
pub use jsonrpc::RpcClient;
pub use network_catalog::NetworkCatalog;
pub use path::encode_segment;
pub use privacy::{EndpointRotator, PrivacyMode};
pub use rate_limit::TokenBucket;
pub use retry::{RetryPolicy, with_retry};

// Rust guideline compliant 2026-05-02
