//! Password hashing with Argon2id.
//!
//! All wallets use uniformly strong parameters to prevent profiling
//! (an attacker cannot distinguish high-value from low-value wallets
//! by observing hash cost).

use argon2::{
    Argon2, Params,
    password_hash::{
        PasswordHash, PasswordHasher as _, PasswordVerifier, SaltString, rand_core::OsRng,
    },
};

use crate::error::{CryptoError, CryptoResult};

/// Trait for password hashing and constant-time verification.
pub trait PasswordHasher: Send + Sync {
    /// Hash a password, returning the PHC-formatted hash string.
    ///
    /// # Errors
    /// Returns `CryptoError::Hasher` if hashing fails.
    fn hash(&self, password: &str) -> CryptoResult<String>;

    /// Verify a password against a stored hash (constant-time).
    ///
    /// # Errors
    /// Returns `CryptoError::Hasher` if the password does not match or the
    /// hash is malformed.
    fn verify(&self, password: &str, hash: &str) -> CryptoResult<()>;

    /// Derive a fixed-length key from a password (for use as encryption key).
    ///
    /// # Errors
    /// Returns `CryptoError::Hasher` if derivation fails.
    fn derive_key(&self, password: &str, salt: &[u8], key_len: usize) -> CryptoResult<Vec<u8>>;
}

/// Argon2id parameter configuration.
///
/// Defaults are split by target:
///
/// - **Native** (server, CLI, tests): 64 MiB × t=3 × p=4 — high cost
///   leveraging multi-threading.
/// - **WASM** (browser): 19 MiB × t=2 × p=1 — OWASP's current minimum
///   for Argon2id. The browser is single-threaded (so `p>1` buys
///   nothing) and 64 MiB-with-no-SIMD takes 5-15s per call on a
///   typical laptop, which makes onboarding feel broken. 19 MiB still
///   forces an attacker to spend ≥19 MiB × t=2 = 38 MiB-seconds per
///   guess, which is well above any cracking economics for a personal
///   wallet password.
#[derive(Debug, Clone)]
pub struct Argon2Config {
    /// Memory cost in KiB.
    pub m_cost: u32,
    /// Number of iterations.
    pub t_cost: u32,
    /// Degree of parallelism (threads).
    pub p_cost: u32,
}

impl Default for Argon2Config {
    fn default() -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            Self {
                m_cost: 19_456, // 19 MiB — OWASP minimum
                t_cost: 2,
                p_cost: 1,
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self {
                m_cost: 65_536, // 64 MiB
                t_cost: 3,
                p_cost: 4,
            }
        }
    }
}

/// Argon2id password hasher (quantum-safe).
///
/// Argon2id combines Argon2i (side-channel resistant) and Argon2d
/// (GPU/ASIC resistant) for the best of both worlds.
#[derive(Default)]
pub struct Argon2Hasher {
    config: Argon2Config,
}

impl Argon2Hasher {
    /// Create a hasher with custom parameters.
    #[must_use]
    pub fn with_config(config: Argon2Config) -> Self {
        Self { config }
    }

    /// Build an `Argon2` instance from the stored config.
    fn argon2(&self) -> CryptoResult<Argon2<'_>> {
        let params = Params::new(
            self.config.m_cost,
            self.config.t_cost,
            self.config.p_cost,
            None,
        )
        .map_err(|e| CryptoError::Hasher(format!("invalid Argon2 params: {e}")))?;
        Ok(Argon2::new(
            argon2::Algorithm::Argon2id,
            argon2::Version::V0x13,
            params,
        ))
    }
}

impl PasswordHasher for Argon2Hasher {
    fn hash(&self, password: &str) -> CryptoResult<String> {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = self.argon2()?;

        let hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| CryptoError::Hasher(format!("hash failed: {e}")))?;

        Ok(hash.to_string())
    }

    fn verify(&self, password: &str, hash: &str) -> CryptoResult<()> {
        let parsed = PasswordHash::new(hash)
            .map_err(|e| CryptoError::Hasher(format!("invalid hash format: {e}")))?;

        // Verification uses parameters embedded in the PHC hash string,
        // so it works regardless of current config (forward-compatible).
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .map_err(|_| CryptoError::Hasher("password does not match".into()))
    }

    fn derive_key(&self, password: &str, salt: &[u8], key_len: usize) -> CryptoResult<Vec<u8>> {
        let mut output = vec![0u8; key_len];

        self.argon2()?
            .hash_password_into(password.as_bytes(), salt, &mut output)
            .map_err(|e| CryptoError::Hasher(format!("key derivation failed: {e}")))?;

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify() {
        let hasher = Argon2Hasher::default();
        let hash = hasher.hash("my-secure-password").expect("hash");
        assert!(hasher.verify("my-secure-password", &hash).is_ok());
    }

    #[test]
    fn wrong_password_rejected() {
        let hasher = Argon2Hasher::default();
        let hash = hasher.hash("correct-password").expect("hash");
        assert!(hasher.verify("wrong-password", &hash).is_err());
    }

    #[test]
    fn derive_32_byte_key() {
        let hasher = Argon2Hasher::default();
        let key = hasher
            .derive_key("password", b"some-salt-value!", 32)
            .expect("derive");
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn same_password_same_derived_key() {
        let hasher = Argon2Hasher::default();
        let k1 = hasher
            .derive_key("pw", b"same-salt-16byte", 32)
            .expect("derive");
        let k2 = hasher
            .derive_key("pw", b"same-salt-16byte", 32)
            .expect("derive");
        assert_eq!(k1, k2);
    }

    #[test]
    fn different_salt_different_key() {
        let hasher = Argon2Hasher::default();
        let k1 = hasher
            .derive_key("pw", b"salt-aaaaaaaaaa!!", 32)
            .expect("derive");
        let k2 = hasher
            .derive_key("pw", b"salt-bbbbbbbbbb!!", 32)
            .expect("derive");
        assert_ne!(k1, k2);
    }
}

// Rust guideline compliant 2026-05-02
