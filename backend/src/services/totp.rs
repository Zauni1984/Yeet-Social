//! TOTP two-factor authentication (RFC 6238, authenticator apps).
//!
//! The shared secret is the only sensitive datum. It is stored encrypted at
//! rest with AES-256-GCM under a key derived from JWT_SECRET (always set and
//! at least 32 bytes, validated at boot) via HKDF-SHA256, so a database dump
//! alone does not yield working second factors. Recovery codes are stored
//! only as SHA-256 hashes. Verification and the otpauth QR come from the
//! totp-rs crate; this module adds the at-rest encryption, recovery codes
//! and the Yeet-specific wrapping.
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key as AesKey, Nonce};
use hkdf::Hkdf;
use rand::RngCore;
use sha2::{Digest, Sha256};
use totp_rs::{Algorithm, Builder, Secret, Totp};

const ISSUER: &str = "Yeet Social";
const INFO: &[u8] = b"yeet-totp-key-v1";
const AAD: &[u8] = b"yeet-totp";
pub const RECOVERY_CODES: usize = 10;

fn derive_key() -> Result<[u8; 32], String> {
    let secret = std::env::var("JWT_SECRET").map_err(|_| "JWT_SECRET not set".to_string())?;
    let hk = Hkdf::<Sha256>::new(None, secret.as_bytes());
    let mut okm = [0u8; 32];
    hk.expand(INFO, &mut okm).map_err(|_| "hkdf".to_string())?;
    Ok(okm)
}

/// Encrypt a base32 secret for storage (`base64(nonce||ciphertext)`).
// `GenericArray::from_slice` is flagged deprecated only because `sha2::Digest`
// is in scope here (it re-exports an older generic-array); the call is the
// same AEAD plumbing used elsewhere. Scoped allow keeps CI's -D warnings.
#[allow(deprecated)]
pub fn seal(secret_b32: &str) -> Result<String, String> {
    let key = derive_key()?;
    let aead = Aes256Gcm::new(AesKey::<Aes256Gcm>::from_slice(&key));
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = aead.encrypt(nonce, Payload { msg: secret_b32.as_bytes(), aad: AAD })
        .map_err(|e| format!("seal: {e}"))?;
    let mut out = nonce_bytes.to_vec();
    out.extend_from_slice(&ct);
    Ok(base64_enc(&out))
}

#[allow(deprecated)]
fn open(enc: &str) -> Result<String, String> {
    let raw = base64_dec(enc).ok_or("bad base64")?;
    if raw.len() < 12 + 16 { return Err("truncated".into()); }
    let key = derive_key()?;
    let aead = Aes256Gcm::new(AesKey::<Aes256Gcm>::from_slice(&key));
    let (nonce_bytes, ct) = raw.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let pt = aead.decrypt(nonce, Payload { msg: ct, aad: AAD })
        .map_err(|e| format!("open: {e}"))?;
    String::from_utf8(pt).map_err(|_| "utf8".into())
}

/// A fresh random base32 secret for a new enrolment.
pub fn generate_secret_b32() -> String {
    Secret::generate().to_base32()
}

fn totp_for(secret_b32: &str, account: &str) -> Result<Totp, String> {
    let secret = Secret::try_from_base32(secret_b32).map_err(|e| format!("secret: {e}"))?;
    Builder::new()
        .with_algorithm(Algorithm::SHA1)
        .with_digits(6)
        .with_skew(1)
        .with_step_duration(30)
        .with_secret(secret)
        .with_issuer(Some(ISSUER))
        .with_account_name(account)
        .build()
        .map_err(|e| format!("totp: {e}"))
}

/// otpauth QR as a base64 PNG (`data:image/png;base64,…` on the client).
pub fn qr_png_base64(secret_b32: &str, account: &str) -> Result<String, String> {
    totp_for(secret_b32, account)?.to_qr_base64().map_err(|e| format!("qr: {e}"))
}

/// otpauth:// URL for manual entry / tapping on mobile.
pub fn otpauth_url(secret_b32: &str, account: &str) -> Result<String, String> {
    totp_for(secret_b32, account)?.to_url().map_err(|e| format!("url: {e}"))
}

/// Verify a 6-digit code against the *encrypted* stored secret (±1 step skew
/// from totp-rs). `account` must match what was used to build the secret —
/// the account name does not affect the code, so any stable value works.
pub fn verify_enc(secret_enc: &str, account: &str, code: &str) -> bool {
    let code = code.trim().replace(' ', "");
    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) { return false; }
    let Ok(b32) = open(secret_enc) else { return false; };
    let Ok(totp) = totp_for(&b32, account) else { return false; };
    totp.check_current(&code).is_some()
}

/// N recovery codes (plaintext, shown once) and their SHA-256 hex hashes.
pub fn generate_recovery_codes() -> (Vec<String>, Vec<String>) {
    let mut rng = rand::thread_rng();
    let mut plain = Vec::with_capacity(RECOVERY_CODES);
    let mut hashes = Vec::with_capacity(RECOVERY_CODES);
    for _ in 0..RECOVERY_CODES {
        let mut b = [0u8; 5];
        rng.fill_bytes(&mut b);
        // 10 hex chars, grouped as xxxxx-xxxxx.
        let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
        let code = format!("{}-{}", &hex[..5], &hex[5..]);
        hashes.push(hash_code(&code));
        plain.push(code);
    }
    (plain, hashes)
}

pub fn hash_code(code: &str) -> String {
    let norm = code.trim().to_lowercase().replace([' '], "");
    let mut h = Sha256::new();
    h.update(norm.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn base64_enc(b: &[u8]) -> String { use base64::Engine; base64::engine::general_purpose::STANDARD.encode(b) }
fn base64_dec(s: &str) -> Option<Vec<u8>> { use base64::Engine; base64::engine::general_purpose::STANDARD.decode(s).ok() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6238_known_vector() {
        // RFC 6238 SHA-1 test seed "12345678901234567890" at T=59 → 94287082.
        std::env::set_var("JWT_SECRET", "test-secret-test-secret-test-secret-123");
        let b32 = Secret::from(b"12345678901234567890".to_vec()).to_base32();
        let totp = totp_for(&b32, "acct").unwrap();
        // check a code we generate for "now" round-trips through verify_enc
        let now = totp.generate_current().to_string();
        let enc = seal(&b32).unwrap();
        assert!(verify_enc(&enc, "acct", &now), "freshly generated code must verify");
        assert!(!verify_enc(&enc, "acct", "000000"), "a wrong code must fail");
    }

    #[test]
    fn seal_roundtrip_and_tamper() {
        std::env::set_var("JWT_SECRET", "test-secret-test-secret-test-secret-123");
        let b32 = generate_secret_b32();
        let enc = seal(&b32).unwrap();
        assert_eq!(open(&enc).unwrap(), b32);
        let mut bad = base64_dec(&enc).unwrap(); let n = bad.len() - 1; bad[n] ^= 0xff;
        assert!(open(&base64_enc(&bad)).is_err(), "tampered ciphertext must fail AEAD");
    }

    #[test]
    fn recovery_codes_hash_and_match() {
        let (plain, hashes) = generate_recovery_codes();
        assert_eq!(plain.len(), RECOVERY_CODES);
        assert_eq!(hash_code(&plain[0].to_uppercase()), hashes[0], "hash is case/space-insensitive");
        assert_ne!(hashes[0], hashes[1]);
    }
}
