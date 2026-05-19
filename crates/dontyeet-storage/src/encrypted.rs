//! Encrypted store — wraps a [`KeyValueBackend`] with a [`Cipher`].
//!
//! Every value is encrypted before writing and decrypted after reading.
//! The encryption key is derived from the user's password via
//! `dontyeet-crypto`'s [`PasswordHasher`].
//!
//! # Container layout
//!
//! Each encrypted value is padded to [`CONTAINER_SIZE`] before encryption,
//! preventing payload-size metadata leakage. The post-decrypt layout is:
//!
//! ```text
//! [1-byte version][4-byte BE length][original data][random padding]
//! ```
//!
//! The version byte lives **inside** the AES-GCM ciphertext, so any
//! tampering trips the auth tag and surfaces as
//! [`StorageError::Encryption`] rather than [`StorageError::UnknownVersion`].
//! See [`CURRENT_VERSION`] for the active value.

use rand::RngCore;
use serde::{Serialize, de::DeserializeOwned};

use dontyeet_crypto::cipher::Cipher;
use dontyeet_crypto::payload::EncryptedPayload;

use crate::backend::KeyValueBackend;
use crate::error::{StorageError, StorageResult};
use crate::serializer::{JsonSerializer, Serializer};

/// Fixed container size for all encrypted payloads.
///
/// Every plaintext is padded to exactly this many bytes before
/// encryption, preventing payload-size metadata leakage.
/// 512 bytes accommodates all stored secrets (mnemonics, salts,
/// password hashes) with room to spare.
const CONTAINER_SIZE: usize = 512;

/// Reserved sentinel meaning "unknown / corrupt / from a future build".
///
/// Never written; refused on read with [`StorageError::UnknownVersion`].
pub const UNKNOWN_VERSION: u8 = 0x00;

/// Currently active payload version.
///
/// Bumped on layout changes inside the encrypted container. The reader
/// tolerates older known versions; unknown values are refused.
///
/// ## Why a 1-byte version inside the AES tag
///
/// The byte sits between the AES-GCM ciphertext bounds, so flipping it
/// trips the auth tag and surfaces as [`StorageError::Encryption`] —
/// distinguishing real corruption from a legitimate forward-version blob.
/// Cost: 1 byte of the 512-byte container, dropping `max_data` from 508
/// to 507. All current callers (mnemonics ≤ 216 bytes, salts, password
/// hashes) sit far below that limit.
pub const CURRENT_VERSION: u8 = 0x01;

/// Number of bytes the container header consumes (version + length).
const HEADER_SIZE: usize = 1 + 4;

/// Check whether `version` is a layout this build can decode.
///
/// As of [`CURRENT_VERSION`] = `0x01`, only that single value is
/// accepted. Future builds extend this match arm when adding a new
/// layout while keeping older readers tolerant.
#[must_use]
fn is_known_version(version: u8) -> bool {
    matches!(version, CURRENT_VERSION)
}

/// Pad `plaintext` into a fixed-size container with a `version` prefix.
///
/// Layout: `[1-byte version][4-byte BE length][data][random padding]`,
/// padded to exactly [`CONTAINER_SIZE`] bytes.
///
/// # Errors
/// Returns [`StorageError::Encryption`] if the plaintext exceeds
/// `CONTAINER_SIZE - HEADER_SIZE` (507 bytes for the current 512-byte
/// container).
fn pad_to_container(plaintext: &[u8], version: u8) -> StorageResult<Vec<u8>> {
    let max_data = CONTAINER_SIZE - HEADER_SIZE;
    if plaintext.len() > max_data {
        return Err(StorageError::Encryption(format!(
            "plaintext ({} bytes) exceeds container capacity ({max_data} bytes)",
            plaintext.len()
        )));
    }

    let len_bytes = u32::try_from(plaintext.len())
        .map_err(|_| StorageError::Encryption("plaintext length exceeds u32".into()))?
        .to_be_bytes();

    let mut container = vec![0u8; CONTAINER_SIZE];
    container[0] = version;
    container[1..HEADER_SIZE].copy_from_slice(&len_bytes);
    container[HEADER_SIZE..HEADER_SIZE + plaintext.len()].copy_from_slice(plaintext);

    // Fill the remaining bytes with random padding.
    rand::thread_rng().fill_bytes(&mut container[HEADER_SIZE + plaintext.len()..]);
    Ok(container)
}

/// Extract the version byte and plaintext from a fixed-size container.
///
/// Reads the 1-byte version + 4-byte length header and returns
/// `(version, data)`. The caller is responsible for refusing
/// unknown versions; this function only checks structural validity.
///
/// # Errors
/// Returns [`StorageError::Encryption`] if the container is too short
/// to hold the header or if the embedded length exceeds the container.
fn unpad_from_container(container: &[u8]) -> StorageResult<(u8, Vec<u8>)> {
    if container.len() < HEADER_SIZE {
        return Err(StorageError::Encryption(
            "padded container too short".into(),
        ));
    }

    let version = container[0];

    let mut len_bytes = [0u8; 4];
    len_bytes.copy_from_slice(&container[1..HEADER_SIZE]);
    let data_len = u32::from_be_bytes(len_bytes) as usize;

    if HEADER_SIZE + data_len > container.len() {
        return Err(StorageError::Encryption(
            "padded container length header exceeds container size".into(),
        ));
    }

    Ok((
        version,
        container[HEADER_SIZE..HEADER_SIZE + data_len].to_vec(),
    ))
}

/// An encrypted key-value store.
///
/// Wraps any [`KeyValueBackend`] and transparently encrypts/decrypts
/// values using the provided [`Cipher`] and encryption key.
pub struct EncryptedStore<B: KeyValueBackend, C: Cipher> {
    backend: B,
    cipher: C,
    serializer: JsonSerializer,
}

impl<B: KeyValueBackend, C: Cipher> EncryptedStore<B, C> {
    /// Create a new encrypted store.
    #[must_use]
    pub fn new(backend: B, cipher: C) -> Self {
        Self {
            backend,
            cipher,
            serializer: JsonSerializer,
        }
    }

    /// Borrow the raw backend for unencrypted reads/writes.
    ///
    /// Use sparingly — only for metadata that must be stored in plaintext
    /// (e.g. bootstrap salts that are needed *before* the encryption key
    /// is available).
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// Store a typed value under `key`, encrypted with `encryption_key`.
    ///
    /// Always writes the [`CURRENT_VERSION`] container layout.
    ///
    /// # Errors
    /// Returns [`StorageError`] if serialization, encryption, or backend
    /// write fails.
    pub async fn set<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        encryption_key: &[u8],
    ) -> StorageResult<()> {
        let plaintext = self.serializer.serialize(value)?;
        let padded = pad_to_container(&plaintext, CURRENT_VERSION)?;

        let payload = self
            .cipher
            .encrypt(&padded, encryption_key)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let payload_bytes = self.serializer.serialize(&payload)?;

        self.backend.set(key, &payload_bytes).await
    }

    /// Read and decrypt a typed value from `key`.
    ///
    /// Returns `None` if the key doesn't exist. Refuses any payload whose
    /// version byte is unknown to this build.
    ///
    /// # Errors
    /// Returns [`StorageError::Backend`] on backend I/O failure,
    /// [`StorageError::Encryption`] on decryption / parse failure, or
    /// [`StorageError::UnknownVersion`] if the decrypted container starts
    /// with a version byte this build does not recognize.
    pub async fn get<T: DeserializeOwned>(
        &self,
        key: &str,
        encryption_key: &[u8],
    ) -> StorageResult<Option<T>> {
        match self.get_versioned(key, encryption_key).await? {
            Some((_version, value)) => Ok(Some(value)),
            None => Ok(None),
        }
    }

    /// Read, decrypt, and report the container version for diagnostics.
    ///
    /// Returns `Some((version, value))` on success. Useful for migration
    /// tooling that needs to distinguish a freshly-written
    /// [`CURRENT_VERSION`] blob from a legacy one written by an older
    /// build.
    ///
    /// # Errors
    /// Returns [`StorageError::Backend`] on backend I/O failure,
    /// [`StorageError::Encryption`] on decryption / parse failure, or
    /// [`StorageError::UnknownVersion`] if the decrypted container starts
    /// with a version byte this build does not recognize.
    pub async fn get_versioned<T: DeserializeOwned>(
        &self,
        key: &str,
        encryption_key: &[u8],
    ) -> StorageResult<Option<(u8, T)>> {
        let Some(payload_bytes) = self.backend.get(key).await? else {
            return Ok(None);
        };

        let payload: EncryptedPayload = self.serializer.deserialize(&payload_bytes)?;

        let padded = self
            .cipher
            .decrypt(&payload, encryption_key)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let (version, plaintext) = unpad_from_container(&padded)?;

        if !is_known_version(version) {
            return Err(StorageError::UnknownVersion(version));
        }

        let value = self.serializer.deserialize(&plaintext)?;
        Ok(Some((version, value)))
    }

    /// Delete a key from the store.
    ///
    /// # Errors
    /// Returns `StorageError::Backend` on I/O failure.
    pub async fn delete(&self, key: &str) -> StorageResult<()> {
        self.backend.delete(key).await
    }

    /// Check whether a key exists.
    ///
    /// # Errors
    /// Returns `StorageError::Backend` on I/O failure.
    pub async fn exists(&self, key: &str) -> StorageResult<bool> {
        Ok(self.backend.get(key).await?.is_some())
    }

    /// List all keys.
    ///
    /// # Errors
    /// Returns `StorageError::Backend` on I/O failure.
    pub async fn list_keys(&self) -> StorageResult<Vec<String>> {
        self.backend.list_keys().await
    }

    /// Delete all keys.
    ///
    /// # Errors
    /// Returns `StorageError::Backend` on I/O failure.
    pub async fn clear(&self) -> StorageResult<()> {
        self.backend.clear().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::KeyValueBackend;
    use async_trait::async_trait;
    use dontyeet_crypto::cipher::AesGcmCipher;
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// In-memory backend for testing.
    struct MemoryBackend {
        data: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MemoryBackend {
        fn new() -> Self {
            Self {
                data: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl KeyValueBackend for MemoryBackend {
        async fn get(&self, key: &str) -> StorageResult<Option<Vec<u8>>> {
            let data = self
                .data
                .lock()
                .map_err(|e| StorageError::Backend(format!("lock poisoned: {e}")))?;
            Ok(data.get(key).cloned())
        }

        async fn set(&self, key: &str, value: &[u8]) -> StorageResult<()> {
            let mut data = self
                .data
                .lock()
                .map_err(|e| StorageError::Backend(format!("lock poisoned: {e}")))?;
            data.insert(key.to_string(), value.to_vec());
            Ok(())
        }

        async fn delete(&self, key: &str) -> StorageResult<()> {
            let mut data = self
                .data
                .lock()
                .map_err(|e| StorageError::Backend(format!("lock poisoned: {e}")))?;
            data.remove(key);
            Ok(())
        }

        async fn list_keys(&self) -> StorageResult<Vec<String>> {
            let data = self
                .data
                .lock()
                .map_err(|e| StorageError::Backend(format!("lock poisoned: {e}")))?;
            Ok(data.keys().cloned().collect())
        }

        async fn clear(&self) -> StorageResult<()> {
            let mut data = self
                .data
                .lock()
                .map_err(|e| StorageError::Backend(format!("lock poisoned: {e}")))?;
            data.clear();
            Ok(())
        }
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Secret {
        seed: String,
        index: u32,
    }

    #[tokio::test]
    async fn encrypted_round_trip() {
        let store = EncryptedStore::new(MemoryBackend::new(), AesGcmCipher);
        let key = [0xABu8; 32];

        let secret = Secret {
            seed: "abandon abandon abandon".into(),
            index: 0,
        };

        store.set("mnemonic", &secret, &key).await.expect("set");

        let loaded: Option<Secret> = store.get("mnemonic", &key).await.expect("get");
        assert_eq!(loaded, Some(secret));
    }

    #[tokio::test]
    async fn wrong_key_fails_decryption() {
        let store = EncryptedStore::new(MemoryBackend::new(), AesGcmCipher);
        let key = [0xABu8; 32];
        let wrong_key = [0xCDu8; 32];

        store.set("secret", &"hello", &key).await.expect("set");

        let result: StorageResult<Option<String>> = store.get("secret", &wrong_key).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn missing_key_returns_none() {
        let store = EncryptedStore::new(MemoryBackend::new(), AesGcmCipher);
        let key = [0xABu8; 32];

        let result: Option<String> = store.get("nonexistent", &key).await.expect("get");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn delete_removes_key() {
        let store = EncryptedStore::new(MemoryBackend::new(), AesGcmCipher);
        let key = [0xABu8; 32];

        store.set("temp", &"value", &key).await.expect("set");
        assert!(store.exists("temp").await.expect("exists"));

        store.delete("temp").await.expect("delete");
        assert!(!store.exists("temp").await.expect("exists"));
    }

    #[tokio::test]
    async fn roundtrip_writes_current_version() {
        let store = EncryptedStore::new(MemoryBackend::new(), AesGcmCipher);
        let key = [0xABu8; 32];

        store.set("k", &"hello", &key).await.expect("set");

        let (version, value) = store
            .get_versioned::<String>("k", &key)
            .await
            .expect("get_versioned")
            .expect("present");

        assert_eq!(version, CURRENT_VERSION);
        assert_eq!(value, "hello");
    }

    #[tokio::test]
    async fn unknown_version_byte_is_refused() {
        // Hand-craft a payload whose decrypted container starts with
        // a future version byte (0xFF). Expect StorageError::UnknownVersion.
        use dontyeet_crypto::cipher::{AesGcmCipher, Cipher};

        let backend = MemoryBackend::new();
        let cipher = AesGcmCipher;
        let key = [0xABu8; 32];

        let mut padded = pad_to_container(b"future build", CURRENT_VERSION).expect("pad");
        padded[0] = 0xFF; // Stamp a forward version *before* encryption.
        let payload = cipher.encrypt(&padded, &key).expect("encrypt");
        let payload_bytes = JsonSerializer.serialize(&payload).expect("serialize");
        backend.set("k", &payload_bytes).await.expect("seed");

        let store = EncryptedStore::new(backend, cipher);
        let result: StorageResult<Option<String>> = store.get("k", &key).await;

        match result {
            Err(StorageError::UnknownVersion(0xFF)) => {}
            other => panic!("expected UnknownVersion(0xFF), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn version_byte_is_inside_auth_tag() {
        // Flipping the version byte in the *ciphertext* (after encrypt)
        // must trip the AES-GCM auth tag, not surface as UnknownVersion.
        use dontyeet_crypto::cipher::{AesGcmCipher, Cipher};

        let backend = MemoryBackend::new();
        let cipher = AesGcmCipher;
        let key = [0xABu8; 32];

        let padded = pad_to_container(b"hello", CURRENT_VERSION).expect("pad");
        let mut payload = cipher.encrypt(&padded, &key).expect("encrypt");

        // Flip a single bit in the first byte of ciphertext.
        if let Some(first) = payload.ciphertext.first_mut() {
            *first ^= 0x01;
        } else {
            panic!("empty ciphertext");
        }

        let payload_bytes = JsonSerializer.serialize(&payload).expect("serialize");
        backend.set("k", &payload_bytes).await.expect("seed");

        let store = EncryptedStore::new(backend, cipher);
        let result: StorageResult<Option<String>> = store.get("k", &key).await;

        match result {
            Err(StorageError::Encryption(_)) => {}
            other => panic!("expected Encryption(_), got {other:?}"),
        }
    }

    #[test]
    fn pad_round_trip_preserves_version_and_data() {
        let plaintext = b"the quick brown fox";
        let padded = pad_to_container(plaintext, CURRENT_VERSION).expect("pad");
        assert_eq!(padded.len(), CONTAINER_SIZE);
        assert_eq!(padded[0], CURRENT_VERSION);

        let (version, recovered) = unpad_from_container(&padded).expect("unpad");
        assert_eq!(version, CURRENT_VERSION);
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn pad_refuses_oversized_plaintext() {
        let oversized = vec![0u8; CONTAINER_SIZE - HEADER_SIZE + 1];
        let result = pad_to_container(&oversized, CURRENT_VERSION);
        assert!(matches!(result, Err(StorageError::Encryption(_))));
    }

    #[test]
    fn pad_accepts_max_plaintext() {
        let max = vec![0u8; CONTAINER_SIZE - HEADER_SIZE];
        assert!(pad_to_container(&max, CURRENT_VERSION).is_ok());
    }

    #[test]
    fn is_known_version_accepts_current_only() {
        assert!(is_known_version(CURRENT_VERSION));
        assert!(!is_known_version(UNKNOWN_VERSION));
        assert!(!is_known_version(0xFF));
    }
}

// Rust guideline compliant 2026-05-02
