//! Account manager — the public API for account lifecycle operations.

use std::sync::Mutex;

use serde::Serialize;
use serde::de::DeserializeOwned;
use zeroize::Zeroizing;

use dontyeet_crypto::Argon2Hasher;
use dontyeet_crypto::cipher::Cipher;
use dontyeet_crypto::hasher::PasswordHasher;
use dontyeet_primitives::Mnemonic;
use dontyeet_storage::{EncryptedStore, KeyValueBackend};

use crate::error::{AccountError, AccountResult};
use crate::mnemonic_repo::MnemonicRepository;
use crate::session::Session;

/// Storage key for the password hash.
const KEY_PASSWORD_HASH: &str = "account:password_hash";
/// Salt used for deriving the encryption key from the password.
const KEY_DERIVATION_SALT: &str = "account:key_salt";
/// Storage key for the per-installation bootstrap salt (stored in plaintext).
const KEY_BOOTSTRAP_SALT: &str = "account:bootstrap_salt";
/// Length of the derived encryption key (AES-256 = 32 bytes).
const ENCRYPTION_KEY_LEN: usize = 32;
/// Length of the random salt for key derivation.
const SALT_LEN: usize = 16;
/// Length of the random bootstrap salt.
const BOOTSTRAP_SALT_LEN: usize = 32;

/// Account lifecycle manager.
///
/// Composes [`EncryptedStore`], [`Argon2Hasher`], and [`Session`] to
/// provide the 6 public account operations.  Thread-safe via internal
/// `Mutex` on the session.
pub struct AccountManager<B: KeyValueBackend, C: Cipher> {
    store: EncryptedStore<B, C>,
    hasher: Argon2Hasher,
    session: Mutex<Session>,
}

impl<B: KeyValueBackend, C: Cipher> AccountManager<B, C> {
    /// Create a new account manager.
    ///
    /// Call [`initialize`](Self::initialize) after construction to sync
    /// the session state with what's in storage.
    #[must_use]
    pub fn new(store: EncryptedStore<B, C>) -> Self {
        Self {
            store,
            hasher: Argon2Hasher::default(),
            session: Mutex::new(Session::no_account()),
        }
    }

    /// Sync session state with storage (call once at startup).
    ///
    /// # Errors
    /// Returns `AccountError::Storage` if the backend is unavailable.
    pub async fn initialize(&self) -> AccountResult<()> {
        let exists = self.store.exists(KEY_PASSWORD_HASH).await?;
        let mut session = self.lock_session()?;
        if exists {
            *session = Session::locked();
        } else {
            *session = Session::no_account();
        }
        Ok(())
    }

    /// Check if an account exists in storage.
    ///
    /// # Errors
    /// Returns `AccountError::Storage` if the backend is unavailable.
    pub async fn exists(&self) -> AccountResult<bool> {
        self.store
            .exists(KEY_PASSWORD_HASH)
            .await
            .map_err(Into::into)
    }

    /// Whether the account is currently unlocked.
    ///
    /// # Errors
    /// Returns `AccountError::Storage` if the session lock is poisoned.
    pub fn is_logged_in(&self) -> AccountResult<bool> {
        Ok(self.lock_session()?.is_unlocked())
    }

    /// Create a new account from a mnemonic and password.
    ///
    /// Transitions session: `NoAccount` → `Unlocked`.
    ///
    /// The user just typed their password, so the session is left
    /// unlocked with the freshly-derived encryption key in memory —
    /// callers should *not* immediately call [`login`](Self::login),
    /// which would re-run the same Argon2 work redundantly.
    ///
    /// # Errors
    /// Returns `AccountError::AlreadyExists` if an account exists,
    /// or storage/crypto errors on failure.
    pub async fn create(&self, mnemonic: &Mnemonic, password: &str) -> AccountResult<()> {
        // Check state
        {
            let session = self.lock_session()?;
            if !session.is_no_account() {
                return Err(AccountError::AlreadyExists);
            }
        }

        // Hash password for verification
        let password_hash = self.hasher.hash(password)?;

        // Generate random per-installation bootstrap salt, then derive keys
        let salt = Self::random_salt();
        self.create_bootstrap_salt().await?;
        let bootstrap_key = self.derive_bootstrap_key(password).await?;
        let encryption_key = self
            .hasher
            .derive_key(password, &salt, ENCRYPTION_KEY_LEN)?;

        // Store password hash and salt under the bootstrap key
        // (so login can read them with just the password)
        self.store
            .set(KEY_PASSWORD_HASH, &password_hash, &bootstrap_key)
            .await?;
        self.store
            .set(KEY_DERIVATION_SALT, &salt.to_vec(), &bootstrap_key)
            .await?;

        // Store mnemonic under the real encryption key
        let repo = MnemonicRepository::new(&self.store);
        repo.set(mnemonic, &encryption_key).await?;

        // Transition NoAccount → Locked → Unlocked, retaining the
        // encryption key we just derived. Skipping this and forcing
        // a follow-up login() would re-run Argon2 three more times.
        {
            let mut session = self.lock_session()?;
            session.mark_created()?;
            session.unlock(encryption_key)?;
        }

        tracing::info!("account created and unlocked");
        Ok(())
    }

    /// Unlock the account with a password.
    ///
    /// Transitions session: `Locked` → `Unlocked`.
    ///
    /// # Errors
    /// Returns `AccountError::NotFound` if no account exists,
    /// `AccountError::WrongPassword` if verification fails.
    pub async fn login(&self, password: &str) -> AccountResult<()> {
        // Verify account exists
        if !self.exists().await? {
            return Err(AccountError::NotFound);
        }

        // Read salt and hash using bootstrap key (derived from password +
        // per-installation salt, falling back to legacy fixed salt).
        let bootstrap_key = self.derive_bootstrap_key(password).await?;

        let salt: Vec<u8> = self
            .store
            .get(KEY_DERIVATION_SALT, &bootstrap_key)
            .await
            .map_err(|_| AccountError::WrongPassword)?
            .ok_or(AccountError::NotFound)?;

        let stored_hash: String = self
            .store
            .get(KEY_PASSWORD_HASH, &bootstrap_key)
            .await
            .map_err(|_| AccountError::WrongPassword)?
            .ok_or(AccountError::NotFound)?;

        // Derive the real encryption key from password + random salt
        let encryption_key = self
            .hasher
            .derive_key(password, &salt, ENCRYPTION_KEY_LEN)?;

        self.hasher
            .verify(password, &stored_hash)
            .map_err(|_| AccountError::WrongPassword)?;

        // Transition state
        self.lock_session()?.unlock(encryption_key)?;

        tracing::info!("account unlocked");
        Ok(())
    }

    /// Lock the account, zeroizing the encryption key in memory.
    ///
    /// Transitions session: `Unlocked` → `Locked`.
    ///
    /// # Errors
    /// Returns `AccountError::NotAuthenticated` if not logged in.
    pub fn logout(&self) -> AccountResult<()> {
        self.lock_session()?.lock()?;
        tracing::info!("account locked");
        Ok(())
    }

    /// Change the account password.
    ///
    /// Re-encrypts the mnemonic with a new key derived from the new password.
    /// Requires the account to be unlocked.
    ///
    /// # Errors
    /// Returns `AccountError::WrongPassword` if `current_password` is wrong,
    /// or `AccountError::NotAuthenticated` if locked.
    pub async fn change_password(
        &self,
        current_password: &str,
        new_password: &str,
    ) -> AccountResult<()> {
        // Read mnemonic with current key
        let current_key = self.lock_session()?.encryption_key()?.to_vec();

        let repo = MnemonicRepository::new(&self.store);
        let mnemonic = repo.get(&current_key).await?;

        // Verify current password via bootstrap key
        let current_bootstrap = self.derive_bootstrap_key(current_password).await?;
        let stored_hash: String = self
            .store
            .get(KEY_PASSWORD_HASH, &current_bootstrap)
            .await?
            .ok_or(AccountError::NotFound)?;

        self.hasher
            .verify(current_password, &stored_hash)
            .map_err(|_| AccountError::WrongPassword)?;

        // Derive new keys with a fresh bootstrap salt
        let new_hash = self.hasher.hash(new_password)?;
        let new_salt = Self::random_salt();
        self.create_bootstrap_salt().await?;
        let new_bootstrap = self.derive_bootstrap_key(new_password).await?;
        let new_key = self
            .hasher
            .derive_key(new_password, &new_salt, ENCRYPTION_KEY_LEN)?;

        // Re-encrypt: hash/salt under new bootstrap, mnemonic under new key
        self.store
            .set(KEY_PASSWORD_HASH, &new_hash, &new_bootstrap)
            .await?;
        self.store
            .set(KEY_DERIVATION_SALT, &new_salt.to_vec(), &new_bootstrap)
            .await?;
        repo.set(&mnemonic, &new_key).await?;

        // Update session with new key
        self.lock_session()?.unlock(new_key)?;

        tracing::info!("password changed");
        Ok(())
    }

    /// Delete the account entirely.
    ///
    /// Requires the password for confirmation. Transitions to `NoAccount`.
    ///
    /// # Errors
    /// Returns `AccountError::WrongPassword` if the password is wrong,
    /// or `AccountError::NotFound` if no account exists.
    pub async fn delete(&self, password: &str) -> AccountResult<()> {
        if !self.exists().await? {
            return Err(AccountError::NotFound);
        }

        // Verify password before destructive operation
        let bootstrap_key = self.derive_bootstrap_key(password).await?;

        let stored_hash: String = self
            .store
            .get(KEY_PASSWORD_HASH, &bootstrap_key)
            .await
            .map_err(|_| AccountError::WrongPassword)?
            .ok_or(AccountError::NotFound)?;

        self.hasher
            .verify(password, &stored_hash)
            .map_err(|_| AccountError::WrongPassword)?;

        // Clear all storage
        self.store.clear().await?;

        // Transition state
        self.lock_session()?.mark_deleted();

        tracing::info!("account deleted");
        Ok(())
    }

    /// Verify the password without changing session state.
    ///
    /// Useful as a secondary gate before sensitive operations
    /// (e.g. exporting the recovery phrase) when the session is
    /// already unlocked.
    ///
    /// # Errors
    /// Returns `AccountError::WrongPassword` if verification fails,
    /// or `AccountError::NotFound` if no account exists.
    pub async fn verify_password(&self, password: &str) -> AccountResult<()> {
        if !self.exists().await? {
            return Err(AccountError::NotFound);
        }

        let bootstrap_key = self.derive_bootstrap_key(password).await?;

        let stored_hash: String = self
            .store
            .get(KEY_PASSWORD_HASH, &bootstrap_key)
            .await
            .map_err(|_| AccountError::WrongPassword)?
            .ok_or(AccountError::NotFound)?;

        self.hasher
            .verify(password, &stored_hash)
            .map_err(|_| AccountError::WrongPassword)?;

        Ok(())
    }

    /// Get the mnemonic (requires unlocked session).
    ///
    /// # Errors
    /// Returns `AccountError::NotAuthenticated` if locked.
    pub async fn get_mnemonic(&self) -> AccountResult<Mnemonic> {
        let key = self.lock_session()?.encryption_key()?.to_vec();
        let repo = MnemonicRepository::new(&self.store);
        repo.get(&key).await
    }

    /// Encrypt and store `value` at `key` using the unlocked session key.
    ///
    /// Typed-storage entry point for consumer crates (notably the UI's
    /// `SecureStore`) that map their own sealed key enums to string keys
    /// without ever holding the session encryption key themselves. The
    /// working copy of the key bytes is wiped on drop via [`Zeroizing`].
    ///
    /// # Errors
    /// Returns [`AccountError::NotAuthenticated`] if the session is locked
    /// or the inactivity timeout has elapsed; [`AccountError::Storage`] on
    /// backend, serialization, or encryption failure.
    pub async fn secure_set<T: Serialize>(&self, key: &str, value: &T) -> AccountResult<()> {
        let encryption_key = Zeroizing::new(self.lock_session()?.encryption_key()?.to_vec());
        self.store
            .set(key, value, &encryption_key)
            .await
            .map_err(Into::into)
    }

    /// Read and decrypt the value at `key` using the unlocked session key.
    ///
    /// Returns `Ok(None)` when nothing is stored at `key`.
    ///
    /// # Errors
    /// Returns [`AccountError::NotAuthenticated`] if the session is locked
    /// or the inactivity timeout has elapsed; [`AccountError::Storage`] on
    /// backend, decryption, or deserialization failure.
    pub async fn secure_get<T: DeserializeOwned>(
        &self,
        key: &str,
    ) -> AccountResult<Option<T>> {
        let encryption_key = Zeroizing::new(self.lock_session()?.encryption_key()?.to_vec());
        self.store
            .get(key, &encryption_key)
            .await
            .map_err(Into::into)
    }

    /// Delete the entry at `key`.
    ///
    /// No-op when the key is absent. Requires the session to be unlocked
    /// even though delete itself does not decrypt anything — this prevents
    /// a locked wallet from mutating the encrypted namespace.
    ///
    /// # Errors
    /// Returns [`AccountError::NotAuthenticated`] if the session is locked
    /// or the inactivity timeout has elapsed; [`AccountError::Storage`] on
    /// backend failure.
    pub async fn secure_delete(&self, key: &str) -> AccountResult<()> {
        self.lock_session()?.encryption_key()?;
        self.store.delete(key).await.map_err(Into::into)
    }

    // -- private helpers --

    fn lock_session(&self) -> AccountResult<std::sync::MutexGuard<'_, Session>> {
        self.session
            .lock()
            .map_err(|e| AccountError::Storage(format!("session lock poisoned: {e}")))
    }

    fn random_salt() -> [u8; SALT_LEN] {
        let mut salt = [0u8; SALT_LEN];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut salt);
        salt
    }

    /// Derive a bootstrap key from password + per-installation salt.
    ///
    /// If a random bootstrap salt was stored (v2 accounts), it is used.
    /// Otherwise falls back to the legacy fixed salt for backwards
    /// compatibility with v1 accounts.
    async fn derive_bootstrap_key(&self, password: &str) -> AccountResult<Vec<u8>> {
        let salt = self.load_or_legacy_bootstrap_salt().await?;
        self.hasher
            .derive_key(password, &salt, ENCRYPTION_KEY_LEN)
            .map_err(Into::into)
    }

    /// Generate and store a random per-installation bootstrap salt.
    async fn create_bootstrap_salt(&self) -> AccountResult<Vec<u8>> {
        let mut salt = vec![0u8; BOOTSTRAP_SALT_LEN];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut salt);
        self.store.backend().set(KEY_BOOTSTRAP_SALT, &salt).await?;
        Ok(salt)
    }

    /// Load the per-installation bootstrap salt, or return the legacy
    /// fixed salt if none exists (v1 accounts).
    async fn load_or_legacy_bootstrap_salt(&self) -> AccountResult<Vec<u8>> {
        const LEGACY_SALT: &[u8; 16] = b"DontYeet-v1-boot";
        match self.store.backend().get(KEY_BOOTSTRAP_SALT).await? {
            Some(salt) if !salt.is_empty() => Ok(salt),
            _ => Ok(LEGACY_SALT.to_vec()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dontyeet_crypto::cipher::AesGcmCipher;
    use dontyeet_crypto::mnemonic::{Bip39Generator, WordCount};
    use dontyeet_storage::KeyValueBackend;
    use std::collections::HashMap;

    /// In-memory backend for testing.
    struct MemBackend(Mutex<HashMap<String, Vec<u8>>>);

    impl MemBackend {
        fn new() -> Self {
            Self(Mutex::new(HashMap::new()))
        }
    }

    #[async_trait::async_trait]
    impl KeyValueBackend for MemBackend {
        async fn get(&self, key: &str) -> dontyeet_storage::StorageResult<Option<Vec<u8>>> {
            Ok(self
                .0
                .lock()
                .map_err(|e| dontyeet_storage::StorageError::Backend(e.to_string()))?
                .get(key)
                .cloned())
        }
        async fn set(&self, key: &str, value: &[u8]) -> dontyeet_storage::StorageResult<()> {
            self.0
                .lock()
                .map_err(|e| dontyeet_storage::StorageError::Backend(e.to_string()))?
                .insert(key.into(), value.to_vec());
            Ok(())
        }
        async fn delete(&self, key: &str) -> dontyeet_storage::StorageResult<()> {
            self.0
                .lock()
                .map_err(|e| dontyeet_storage::StorageError::Backend(e.to_string()))?
                .remove(key);
            Ok(())
        }
        async fn list_keys(&self) -> dontyeet_storage::StorageResult<Vec<String>> {
            Ok(self
                .0
                .lock()
                .map_err(|e| dontyeet_storage::StorageError::Backend(e.to_string()))?
                .keys()
                .cloned()
                .collect())
        }
        async fn clear(&self) -> dontyeet_storage::StorageResult<()> {
            self.0
                .lock()
                .map_err(|e| dontyeet_storage::StorageError::Backend(e.to_string()))?
                .clear();
            Ok(())
        }
    }

    fn make_manager() -> AccountManager<MemBackend, AesGcmCipher> {
        let store = EncryptedStore::new(MemBackend::new(), AesGcmCipher);
        AccountManager::new(store)
    }

    fn test_mnemonic() -> Mnemonic {
        Bip39Generator::generate(WordCount::Twelve).expect("mnemonic")
    }

    #[tokio::test]
    async fn create_leaves_unlocked_then_relogin_works() {
        let mgr = make_manager();
        let mnemonic = test_mnemonic();

        mgr.create(&mnemonic, "password123").await.expect("create");
        assert!(mgr.exists().await.expect("exists"));
        // create now leaves the session unlocked — no extra login needed.
        assert!(mgr.is_logged_in().expect("check"));

        // Logout + login round-trip still works.
        mgr.logout().expect("logout");
        assert!(!mgr.is_logged_in().expect("check"));
        mgr.login("password123").await.expect("login");
        assert!(mgr.is_logged_in().expect("check"));
    }

    #[tokio::test]
    async fn create_when_exists_fails() {
        let mgr = make_manager();
        let m = test_mnemonic();

        mgr.create(&m, "pw").await.expect("create");
        let result = mgr.create(&m, "pw").await;
        assert!(matches!(result, Err(AccountError::AlreadyExists)));
    }

    #[tokio::test]
    async fn login_wrong_password() {
        let mgr = make_manager();
        mgr.create(&test_mnemonic(), "correct")
            .await
            .expect("create");

        let result = mgr.login("wrong").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn mnemonic_requires_login_after_logout() {
        let mgr = make_manager();
        mgr.create(&test_mnemonic(), "pw").await.expect("create");

        // create leaves the session unlocked — read works.
        let _ = mgr.get_mnemonic().await.expect("get mnemonic");

        // After logout, the mnemonic is gated again.
        mgr.logout().expect("logout");
        assert!(mgr.get_mnemonic().await.is_err());

        // Login — should succeed
        mgr.login("pw").await.expect("login");
        let _ = mgr.get_mnemonic().await.expect("get mnemonic");
    }

    #[tokio::test]
    async fn mnemonic_survives_round_trip() {
        let mgr = make_manager();
        let original = test_mnemonic();
        let phrase = original.as_str().to_string();

        mgr.create(&original, "pw").await.expect("create");
        mgr.login("pw").await.expect("login");

        let loaded = mgr.get_mnemonic().await.expect("get");
        assert_eq!(loaded.as_str(), phrase);
    }

    #[tokio::test]
    async fn logout_locks_session() {
        let mgr = make_manager();
        mgr.create(&test_mnemonic(), "pw").await.expect("create");
        mgr.login("pw").await.expect("login");
        assert!(mgr.is_logged_in().expect("check"));

        mgr.logout().expect("logout");
        assert!(!mgr.is_logged_in().expect("check"));
        assert!(mgr.get_mnemonic().await.is_err());
    }

    #[tokio::test]
    async fn change_password() {
        let mgr = make_manager();
        let m = test_mnemonic();
        let phrase = m.as_str().to_string();

        mgr.create(&m, "old-pw").await.expect("create");
        mgr.login("old-pw").await.expect("login");

        mgr.change_password("old-pw", "new-pw")
            .await
            .expect("change");
        mgr.logout().expect("logout");

        // Old password should fail
        assert!(mgr.login("old-pw").await.is_err());

        // New password should work and mnemonic should be intact
        mgr.login("new-pw").await.expect("login");
        let loaded = mgr.get_mnemonic().await.expect("get");
        assert_eq!(loaded.as_str(), phrase);
    }

    #[tokio::test]
    async fn delete_clears_everything() {
        let mgr = make_manager();
        mgr.create(&test_mnemonic(), "pw").await.expect("create");
        assert!(mgr.exists().await.expect("exists"));

        mgr.delete("pw").await.expect("delete");
        assert!(!mgr.exists().await.expect("exists"));
    }

    #[tokio::test]
    async fn secure_set_get_round_trip() {
        let mgr = make_manager();
        mgr.create(&test_mnemonic(), "pw").await.expect("create");

        mgr.secure_set("ui:greeting", &"hello".to_string())
            .await
            .expect("set");
        let got: Option<String> = mgr.secure_get("ui:greeting").await.expect("get");
        assert_eq!(got.as_deref(), Some("hello"));
    }

    #[tokio::test]
    async fn secure_get_missing_returns_none() {
        let mgr = make_manager();
        mgr.create(&test_mnemonic(), "pw").await.expect("create");

        let got: Option<String> = mgr.secure_get("ui:absent").await.expect("get");
        assert_eq!(got, None);
    }

    #[tokio::test]
    async fn secure_delete_removes_value() {
        let mgr = make_manager();
        mgr.create(&test_mnemonic(), "pw").await.expect("create");

        mgr.secure_set("ui:doomed", &42u32).await.expect("set");
        mgr.secure_delete("ui:doomed").await.expect("delete");
        let got: Option<u32> = mgr.secure_get("ui:doomed").await.expect("get");
        assert_eq!(got, None);
    }

    #[tokio::test]
    async fn secure_ops_require_unlocked_session() {
        let mgr = make_manager();
        mgr.create(&test_mnemonic(), "pw").await.expect("create");
        mgr.logout().expect("logout");

        assert!(matches!(
            mgr.secure_set("ui:locked", &"x".to_string()).await,
            Err(AccountError::NotAuthenticated)
        ));
        assert!(matches!(
            mgr.secure_get::<String>("ui:locked").await,
            Err(AccountError::NotAuthenticated)
        ));
        assert!(matches!(
            mgr.secure_delete("ui:locked").await,
            Err(AccountError::NotAuthenticated)
        ));
    }

    #[tokio::test]
    async fn secure_round_trip_with_struct() {
        use serde::{Deserialize, Serialize};

        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct Sample {
            n: u64,
            s: String,
        }

        let mgr = make_manager();
        mgr.create(&test_mnemonic(), "pw").await.expect("create");

        let sample = Sample {
            n: 7,
            s: "world".into(),
        };
        mgr.secure_set("ui:struct", &sample).await.expect("set");
        let got: Option<Sample> = mgr.secure_get("ui:struct").await.expect("get");
        assert_eq!(
            got,
            Some(Sample {
                n: 7,
                s: "world".into()
            })
        );
    }
}

// Rust guideline compliant 2026-05-02
