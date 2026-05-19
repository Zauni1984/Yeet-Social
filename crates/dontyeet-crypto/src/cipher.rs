//! Encryption at rest: AES-256-GCM and hybrid ML-KEM + AES-256-GCM.

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use rand::RngCore;
use zeroize::Zeroize;

use crate::error::{CryptoError, CryptoResult};
use crate::payload::{CipherAlgorithm, EncryptedPayload};

/// Trait for symmetric encrypt/decrypt of arbitrary bytes.
pub trait Cipher: Send + Sync {
    /// Encrypt plaintext with the given key.
    ///
    /// # Errors
    /// Returns `CryptoError::Cipher` if encryption fails.
    fn encrypt(&self, plaintext: &[u8], key: &[u8]) -> CryptoResult<EncryptedPayload>;

    /// Decrypt a payload with the given key.
    ///
    /// # Errors
    /// Returns `CryptoError::Cipher` if decryption fails (wrong key, tampered data, etc.).
    fn decrypt(&self, payload: &EncryptedPayload, key: &[u8]) -> CryptoResult<Vec<u8>>;
}

// ---------------------------------------------------------------------------
// AES-256-GCM (quantum-safe symmetric, authenticated)
// ---------------------------------------------------------------------------

/// AES-256-GCM authenticated encryption.
///
/// Provides confidentiality **and** integrity — any tampering is detected
/// on decryption.  AES-256 is considered quantum-safe (Grover's algorithm
/// reduces effective security to 128-bit, which is still strong).
pub struct AesGcmCipher;

impl Cipher for AesGcmCipher {
    fn encrypt(&self, plaintext: &[u8], key: &[u8]) -> CryptoResult<EncryptedPayload> {
        let key_array: [u8; 32] = key
            .try_into()
            .map_err(|_| CryptoError::Cipher("AES-256-GCM requires a 32-byte key".into()))?;

        let cipher = Aes256Gcm::new(&key_array.into());

        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| CryptoError::Cipher(format!("AES-GCM encrypt failed: {e}")))?;

        Ok(EncryptedPayload {
            algorithm: CipherAlgorithm::Aes256Gcm,
            nonce: nonce_bytes.to_vec(),
            ciphertext,
            kem_ciphertext: None,
        })
    }

    fn decrypt(&self, payload: &EncryptedPayload, key: &[u8]) -> CryptoResult<Vec<u8>> {
        let key_array: [u8; 32] = key
            .try_into()
            .map_err(|_| CryptoError::Cipher("AES-256-GCM requires a 32-byte key".into()))?;

        let cipher = Aes256Gcm::new(&key_array.into());

        let nonce = Nonce::from_slice(&payload.nonce);

        cipher
            .decrypt(nonce, payload.ciphertext.as_ref())
            .map_err(|e| CryptoError::Cipher(format!("AES-GCM decrypt failed: {e}")))
    }
}

// ---------------------------------------------------------------------------
// Hybrid ML-KEM-1024 + AES-256-GCM (post-quantum)
// ---------------------------------------------------------------------------

/// Expected encapsulation key length for ML-KEM-1024 (bytes).
const MLKEM1024_EK_LEN: usize = 1568;
/// Expected decapsulation key length for ML-KEM-1024 (bytes).
const MLKEM1024_DK_LEN: usize = 3168;

/// Hybrid post-quantum cipher: ML-KEM-1024 key encapsulation + AES-256-GCM.
///
/// **Encrypt flow:**
/// 1. ML-KEM encapsulate → (`shared_secret`, `kem_ciphertext`)
/// 2. Derive AES key = SHA3-256(`user_key` || `shared_secret`)
/// 3. AES-256-GCM encrypt with derived key
///
/// **Decrypt flow:**
/// 1. ML-KEM decapsulate `kem_ciphertext` → `shared_secret`
/// 2. Derive AES key = SHA3-256(`user_key` || `shared_secret`)
/// 3. AES-256-GCM decrypt
///
/// Even if ML-KEM is broken, the `user_key` component of the derived key
/// still provides classical AES-256 security.  Even if AES is weakened by
/// quantum advances, ML-KEM protects the shared secret.  Belt and suspenders.
pub struct HybridCipher {
    /// ML-KEM-1024 encapsulation key (public key), serialised.
    encapsulation_key: Vec<u8>,
    /// ML-KEM-1024 decapsulation key (private key), serialised.
    decapsulation_key: Vec<u8>,
}

impl HybridCipher {
    /// Generate a new ML-KEM-1024 keypair for hybrid encryption.
    #[must_use]
    pub fn generate() -> Self {
        use ml_kem::{EncodedSizeUser, KemCore, MlKem1024};

        let mut rng = rand::thread_rng();
        let (dk, ek) = MlKem1024::generate(&mut rng);

        Self {
            encapsulation_key: ek.as_bytes().to_vec(),
            decapsulation_key: dk.as_bytes().to_vec(),
        }
    }

    /// Reconstruct from previously saved key material.
    ///
    /// # Errors
    /// Returns `CryptoError::PostQuantum` if the key bytes are invalid.
    pub fn from_keys(encapsulation_key: Vec<u8>, decapsulation_key: Vec<u8>) -> CryptoResult<Self> {
        if encapsulation_key.len() != MLKEM1024_EK_LEN {
            return Err(CryptoError::PostQuantum(format!(
                "encapsulation key must be {MLKEM1024_EK_LEN} bytes, got {}",
                encapsulation_key.len()
            )));
        }
        if decapsulation_key.len() != MLKEM1024_DK_LEN {
            return Err(CryptoError::PostQuantum(format!(
                "decapsulation key must be {MLKEM1024_DK_LEN} bytes, got {}",
                decapsulation_key.len()
            )));
        }
        Ok(Self {
            encapsulation_key,
            decapsulation_key,
        })
    }

    /// The public encapsulation key (safe to store alongside encrypted data).
    #[must_use]
    pub fn encapsulation_key(&self) -> &[u8] {
        &self.encapsulation_key
    }

    /// Derive AES-256 key from user key + ML-KEM shared secret using
    /// HKDF-SHA3-256 (extract-then-expand per NIST SP 800-56C).
    ///
    /// - **Extract:** `PRK = HKDF-Extract(salt=shared_secret, IKM=user_key)`
    /// - **Expand:** `OKM = HKDF-Expand(PRK, info="dontyeet-hybrid-v1", L=32)`
    fn derive_aes_key(user_key: &[u8], shared_secret: &[u8]) -> [u8; 32] {
        use hkdf::Hkdf;

        let hk = Hkdf::<sha3::Sha3_256>::new(Some(shared_secret), user_key);
        let mut key = [0u8; 32];
        // expand cannot fail when output length <= 255 * hash_len (32 * 255 = 8160).
        hk.expand(b"dontyeet-hybrid-v1", &mut key)
            .expect("HKDF expand: 32 bytes is within bounds");
        key
    }
}

impl Cipher for HybridCipher {
    fn encrypt(&self, plaintext: &[u8], user_key: &[u8]) -> CryptoResult<EncryptedPayload> {
        use ml_kem::kem::{Encapsulate, EncapsulationKey};
        use ml_kem::{EncodedSizeUser, MlKem1024Params};

        let ek_bytes: &ml_kem::Encoded<EncapsulationKey<MlKem1024Params>> = self
            .encapsulation_key
            .as_slice()
            .try_into()
            .map_err(|_| CryptoError::PostQuantum("invalid encapsulation key length".into()))?;
        let ek = EncapsulationKey::<MlKem1024Params>::from_bytes(ek_bytes);

        let mut rng = rand::thread_rng();
        let (kem_ct, shared_secret) = ek
            .encapsulate(&mut rng)
            .map_err(|()| CryptoError::PostQuantum("ML-KEM encapsulate failed".into()))?;

        let mut aes_key = Self::derive_aes_key(user_key, &shared_secret);

        let aes_cipher = Aes256Gcm::new(&aes_key.into());
        aes_key.zeroize();

        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = aes_cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| CryptoError::Cipher(format!("hybrid encrypt failed: {e}")))?;

        Ok(EncryptedPayload {
            algorithm: CipherAlgorithm::HybridMlKemAes256Gcm,
            nonce: nonce_bytes.to_vec(),
            ciphertext,
            kem_ciphertext: Some(kem_ct.to_vec()),
        })
    }

    fn decrypt(&self, payload: &EncryptedPayload, user_key: &[u8]) -> CryptoResult<Vec<u8>> {
        use ml_kem::kem::{Decapsulate, DecapsulationKey};
        use ml_kem::{EncodedSizeUser, MlKem1024, MlKem1024Params};

        let kem_ct_bytes = payload
            .kem_ciphertext
            .as_ref()
            .ok_or_else(|| CryptoError::PostQuantum("missing KEM ciphertext in payload".into()))?;

        let dk_bytes: &ml_kem::Encoded<DecapsulationKey<MlKem1024Params>> = self
            .decapsulation_key
            .as_slice()
            .try_into()
            .map_err(|_| CryptoError::PostQuantum("invalid decapsulation key length".into()))?;
        let dk = DecapsulationKey::<MlKem1024Params>::from_bytes(dk_bytes);

        let kem_ct: &ml_kem::Ciphertext<MlKem1024> = kem_ct_bytes
            .as_slice()
            .try_into()
            .map_err(|_| CryptoError::PostQuantum("invalid KEM ciphertext length".into()))?;

        let shared_secret = dk
            .decapsulate(kem_ct)
            .map_err(|()| CryptoError::PostQuantum("ML-KEM decapsulate failed".into()))?;

        let mut aes_key = Self::derive_aes_key(user_key, &shared_secret);

        let aes_cipher = Aes256Gcm::new(&aes_key.into());
        aes_key.zeroize();

        let nonce = Nonce::from_slice(&payload.nonce);

        aes_cipher
            .decrypt(nonce, payload.ciphertext.as_ref())
            .map_err(|e| CryptoError::Cipher(format!("hybrid decrypt failed: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes_gcm_round_trip() {
        let cipher = AesGcmCipher;
        let key = [0xABu8; 32];
        let plaintext = b"master seed bytes here";

        let payload = cipher.encrypt(plaintext, &key).expect("encrypt");
        assert_eq!(payload.algorithm, CipherAlgorithm::Aes256Gcm);
        assert!(payload.kem_ciphertext.is_none());

        let decrypted = cipher.decrypt(&payload, &key).expect("decrypt");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn aes_gcm_wrong_key_fails() {
        let cipher = AesGcmCipher;
        let key = [0xABu8; 32];
        let wrong_key = [0xCDu8; 32];

        let payload = cipher.encrypt(b"secret", &key).expect("encrypt");
        assert!(cipher.decrypt(&payload, &wrong_key).is_err());
    }

    #[test]
    fn aes_gcm_tamper_detected() {
        let cipher = AesGcmCipher;
        let key = [0xABu8; 32];

        let mut payload = cipher.encrypt(b"secret", &key).expect("encrypt");
        // Flip a byte in ciphertext
        if let Some(byte) = payload.ciphertext.first_mut() {
            *byte ^= 0xFF;
        }
        assert!(cipher.decrypt(&payload, &key).is_err());
    }

    #[test]
    fn hybrid_round_trip() {
        let hybrid = HybridCipher::generate();
        let user_key = b"user-password-derived-key-bytes!!"; // 32 bytes

        let plaintext = b"this is my master seed";
        let payload = hybrid.encrypt(plaintext, user_key).expect("encrypt");

        assert_eq!(payload.algorithm, CipherAlgorithm::HybridMlKemAes256Gcm);
        assert!(payload.kem_ciphertext.is_some());

        let decrypted = hybrid.decrypt(&payload, user_key).expect("decrypt");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn hybrid_wrong_user_key_fails() {
        let hybrid = HybridCipher::generate();
        let payload = hybrid
            .encrypt(b"secret", b"correct-key-is-32-bytes-long!!!!")
            .expect("encrypt");
        assert!(
            hybrid
                .decrypt(&payload, b"wrong-key-is-also-32-bytes-long!")
                .is_err()
        );
    }

    #[test]
    fn from_keys_validates_ek_length() {
        let result = HybridCipher::from_keys(vec![0u8; 100], vec![0u8; MLKEM1024_DK_LEN]);
        let err = result.err().expect("should fail");
        assert!(err.to_string().contains("encapsulation key must be"));
    }

    #[test]
    fn from_keys_validates_dk_length() {
        let result = HybridCipher::from_keys(vec![0u8; MLKEM1024_EK_LEN], vec![0u8; 100]);
        let err = result.err().expect("should fail");
        assert!(err.to_string().contains("decapsulation key must be"));
    }

    #[test]
    fn from_keys_accepts_correct_lengths() {
        let hybrid = HybridCipher::generate();
        let result = HybridCipher::from_keys(
            hybrid.encapsulation_key.clone(),
            hybrid.decapsulation_key.clone(),
        );
        assert!(result.is_ok());
    }
}

// Rust guideline compliant 2026-05-02
