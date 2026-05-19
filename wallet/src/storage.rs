//! Browser-localStorage [`KeyValueBackend`] for the DontYeetWallet shim.
//!
//! Lives in this crate because the upstream engine's equivalent backend is
//! `pub(crate)` (locked behind a typed storage-tier API), but
//! [`dontyeet_account::AccountManager`] needs a [`KeyValueBackend`] from
//! outside that crate. The wire format here is a deliberate match of the
//! upstream backend's wire format — base64-encoded values under a single
//! namespace prefix — so a future migration into the engine is a drop-in.
//!
//! Keeping wallet data namespaced isolates it from anything else the page
//! writes to localStorage and scopes [`KeyValueBackend::clear`] to
//! wallet-owned data only (a raw `Storage::clear()` would also delete
//! unrelated entries owned by the host page).
//!
//! All operations are synchronous under the hood — `localStorage` is a
//! synchronous Web API — and return a future only to satisfy the async
//! trait contract.

// Brand names (DontYeetWallet) read as prose in this module's narrative docs.
#![expect(
    clippy::doc_markdown,
    reason = "brand names are intentionally written without backticks in prose"
)]

use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use dontyeet_storage::{KeyValueBackend, StorageError, StorageResult};

/// localStorage namespace for wallet entries.
///
/// All key-value pairs the wallet writes are prefixed by this constant.
/// This both isolates wallet data from other page state and bounds the
/// blast radius of [`KeyValueBackend::clear`] to wallet-owned keys.
///
/// Treat as a wire-format constant: renaming it strands every existing
/// user's encrypted mnemonic, so any change must ship with a migration.
const STORAGE_PREFIX: &str = "dontyeet:";

/// [`KeyValueBackend`] backed by `window.localStorage`.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct BrowserStorage;

impl BrowserStorage {
    /// Construct a new handle. Cheap — no allocations, no API calls.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl KeyValueBackend for BrowserStorage {
    async fn get(&self, key: &str) -> StorageResult<Option<Vec<u8>>> {
        let storage = local_storage()?;
        let prefixed = prefixed(key);
        let entry = storage
            .get_item(&prefixed)
            .map_err(|e| backend_err("get_item", &e))?;
        match entry {
            None => Ok(None),
            Some(b64) => {
                let bytes = BASE64
                    .decode(b64.as_bytes())
                    .map_err(|e| StorageError::Backend(format!("base64 decode: {e}")))?;
                Ok(Some(bytes))
            }
        }
    }

    async fn set(&self, key: &str, value: &[u8]) -> StorageResult<()> {
        let storage = local_storage()?;
        let prefixed = prefixed(key);
        let encoded = BASE64.encode(value);
        storage
            .set_item(&prefixed, &encoded)
            .map_err(|e| backend_err("set_item", &e))
    }

    async fn delete(&self, key: &str) -> StorageResult<()> {
        let storage = local_storage()?;
        let prefixed = prefixed(key);
        storage
            .remove_item(&prefixed)
            .map_err(|e| backend_err("remove_item", &e))
    }

    async fn list_keys(&self) -> StorageResult<Vec<String>> {
        let storage = local_storage()?;
        let len = storage.length().map_err(|e| backend_err("length", &e))?;
        let mut out = Vec::new();
        for i in 0..len {
            let key = storage.key(i).map_err(|e| backend_err("key", &e))?;
            if let Some(k) = key.as_deref().and_then(|k| k.strip_prefix(STORAGE_PREFIX)) {
                out.push(k.to_string());
            }
        }
        Ok(out)
    }

    async fn clear(&self) -> StorageResult<()> {
        let storage = local_storage()?;
        let len = storage.length().map_err(|e| backend_err("length", &e))?;
        // Snapshot first because removal during iteration shifts indices
        // and would skip entries.
        let mut targets = Vec::new();
        for i in 0..len {
            if let Some(k) = storage.key(i).map_err(|e| backend_err("key", &e))?
                && k.starts_with(STORAGE_PREFIX)
            {
                targets.push(k);
            }
        }
        for k in targets {
            storage
                .remove_item(&k)
                .map_err(|e| backend_err("remove_item", &e))?;
        }
        Ok(())
    }
}

/// Compose the namespaced key written to localStorage.
fn prefixed(key: &str) -> String {
    format!("{STORAGE_PREFIX}{key}")
}

/// Resolve the `window.localStorage` handle.
fn local_storage() -> StorageResult<web_sys::Storage> {
    let window =
        web_sys::window().ok_or_else(|| StorageError::Backend("no window object".into()))?;
    window
        .local_storage()
        .map_err(|e| backend_err("window.localStorage access", &e))?
        .ok_or_else(|| StorageError::Backend("localStorage is disabled".into()))
}

/// Wrap a JS-side error from a localStorage call as a [`StorageError`].
fn backend_err(op: &str, e: &wasm_bindgen::JsValue) -> StorageError {
    StorageError::Backend(format!("{op}: {e:?}"))
}

// Rust guideline compliant 2026-02-21
