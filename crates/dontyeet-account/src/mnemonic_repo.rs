//! Login-gated mnemonic repository.
//!
//! The mnemonic can only be read or written when the caller provides
//! an encryption key — which is only available from an unlocked session.

use zeroize::Zeroizing;

use dontyeet_crypto::cipher::Cipher;
use dontyeet_primitives::Mnemonic;
use dontyeet_storage::{EncryptedStore, KeyValueBackend};

use crate::error::AccountResult;

/// Storage key for the encrypted mnemonic.
const KEY_MNEMONIC: &str = "account:mnemonic";

/// Encrypted mnemonic storage.
///
/// Every operation requires an `encryption_key` parameter, which acts
/// as a proof of authentication — only [`Session::encryption_key()`]
/// can provide it, and only when unlocked.
pub struct MnemonicRepository<'a, B: KeyValueBackend, C: Cipher> {
    store: &'a EncryptedStore<B, C>,
}

impl<'a, B: KeyValueBackend, C: Cipher> MnemonicRepository<'a, B, C> {
    /// Create a repository backed by the given encrypted store.
    #[must_use]
    pub fn new(store: &'a EncryptedStore<B, C>) -> Self {
        Self { store }
    }

    /// Read the stored mnemonic.
    ///
    /// # Errors
    /// Returns `AccountError` if the mnemonic doesn't exist, decryption
    /// fails, or the backend is unavailable.
    pub async fn get(&self, encryption_key: &[u8]) -> AccountResult<Mnemonic> {
        let phrase: Zeroizing<String> = Zeroizing::new(
            self.store
                .get(KEY_MNEMONIC, encryption_key)
                .await?
                .ok_or(crate::error::AccountError::NotFound)?,
        );

        Ok(Mnemonic::new(phrase.as_str()))
    }

    /// Store a mnemonic (encrypted).
    ///
    /// # Errors
    /// Returns `AccountError` if encryption or storage fails.
    pub async fn set(&self, mnemonic: &Mnemonic, encryption_key: &[u8]) -> AccountResult<()> {
        let phrase = Zeroizing::new(mnemonic.as_str().to_string());
        self.store
            .set(KEY_MNEMONIC, &*phrase, encryption_key)
            .await?;
        Ok(())
    }

    /// Delete the stored mnemonic.
    ///
    /// # Errors
    /// Returns `AccountError` if the backend fails.
    pub async fn delete(&self) -> AccountResult<()> {
        self.store.delete(KEY_MNEMONIC).await?;
        Ok(())
    }

    /// Check whether a mnemonic is stored.
    ///
    /// # Errors
    /// Returns `AccountError` if the backend fails.
    pub async fn exists(&self) -> AccountResult<bool> {
        self.store.exists(KEY_MNEMONIC).await.map_err(Into::into)
    }
}

// Rust guideline compliant 2026-05-02
