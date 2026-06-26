//! Encrypted at-rest storage for highly-sensitive PII blobs
//! (currently: age-verification face scans + ID document scans).
//!
//! Properties
//! ──────────
//! * Files live under PRIVATE_DIR (env, default /app/private), which
//!   is explicitly NOT under UPLOADS_DIR — no ServeDir mount touches
//!   it, no nginx location proxies it.
//! * Each blob is AES-GCM-encrypted with a per-file key derived via
//!   HKDF-SHA256 from a server-side master (env AGE_VERIFY_KEY,
//!   32 raw bytes base64-encoded). The `info` string binds the
//!   derivation to the case + slot ("face" or "id"), so a leaked
//!   ciphertext for one slot can't be replayed as another's.
//! * The 12-byte nonce is randomly generated per write and prepended
//!   to the on-disk file: layout is `[nonce(12) | ciphertext+tag]`.
//! * If AGE_VERIFY_KEY is unset the writer FAILS-CLOSED with a clear
//!   error rather than writing plaintext — operators must configure
//!   it before age verification is functional.
//! * `purge` overwrites the file with zeros before unlinking so a
//!   later disk-block recovery yields nothing usable from a deleted
//!   case's blobs.

// The aes-gcm 0.10 + sha2 0.10 we're already pinned to bundle an old
// generic-array; bumping the curve crates is out of scope for this
// feature. The deprecation is a future-API hint, not a soundness
// issue — silence it module-wide.
#![allow(deprecated)]

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key as AesKey, Nonce};
use base64::Engine;
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

const NONCE_LEN: usize = 12;

pub fn private_dir() -> PathBuf {
    PathBuf::from(std::env::var("PRIVATE_DIR").unwrap_or_else(|_| "/app/private".into()))
}

fn master_key() -> Result<[u8; 32], String> {
    let raw = std::env::var("AGE_VERIFY_KEY")
        .map_err(|_| "AGE_VERIFY_KEY env var is not set".to_string())?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw.trim())
        .map_err(|e| format!("AGE_VERIFY_KEY base64 decode failed: {e}"))?;
    if bytes.len() != 32 {
        return Err(format!("AGE_VERIFY_KEY must decode to exactly 32 bytes, got {}", bytes.len()));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// True if the operator has set AGE_VERIFY_KEY. Useful for an /admin
/// readiness probe — until this returns true, the verification flow is
/// disabled at the submit endpoint with a clear "not configured" error.
pub fn is_configured() -> bool {
    master_key().is_ok()
}

/// Derive the per-blob AES-256 key. The `slot` identifier ("face" or
/// "id") binds the derivation so swapping one ciphertext for the other
/// at rest decrypts to garbage and AEAD authentication fails closed.
fn derive_key(case_id: Uuid, slot: &str) -> Result<[u8; 32], String> {
    let master = master_key()?;
    let hk = Hkdf::<Sha256>::new(None, &master);
    let mut out = [0u8; 32];
    let info = format!("yeet-age-verify-v1|{case_id}|{slot}");
    hk.expand(info.as_bytes(), &mut out)
        .map_err(|e| format!("HKDF expand failed: {e}"))?;
    Ok(out)
}

/// Write an encrypted blob to PRIVATE_DIR/age-verification/<case>/<slot>.bin
/// and return the relative path actually written.
pub async fn write_blob(case_id: Uuid, slot: &str, plaintext: &[u8]) -> Result<String, String> {
    if !matches!(slot, "face" | "id") {
        return Err("invalid slot".into());
    }
    let key = derive_key(case_id, slot)?;
    let aead = Aes256Gcm::new(AesKey::<Aes256Gcm>::from_slice(&key));

    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    // Authenticate the case_id + slot in associated data so a moved
    // file (e.g. copied across cases) fails to decrypt.
    let aad = format!("{case_id}|{slot}");
    let ct = aead
        .encrypt(nonce, Payload { msg: plaintext, aad: aad.as_bytes() })
        .map_err(|e| format!("aead encrypt: {e}"))?;

    let rel = format!("age-verification/{case_id}/{slot}.bin");
    let abs = private_dir().join(&rel);
    let parent = abs.parent().ok_or_else(|| "no parent dir".to_string())?;
    tokio::fs::create_dir_all(parent).await.map_err(|e| format!("mkdir: {e}"))?;

    // Atomic write: write to .tmp first then rename, so a concurrent
    // read never sees a half-written blob.
    let tmp = abs.with_extension("bin.tmp");
    {
        let mut f = tokio::fs::File::create(&tmp).await.map_err(|e| format!("create: {e}"))?;
        f.write_all(&nonce_bytes).await.map_err(|e| format!("write nonce: {e}"))?;
        f.write_all(&ct).await.map_err(|e| format!("write ct: {e}"))?;
        f.flush().await.map_err(|e| format!("flush: {e}"))?;
    }
    tokio::fs::rename(&tmp, &abs).await.map_err(|e| format!("rename: {e}"))?;
    Ok(rel)
}

/// Read + decrypt an existing blob. The same (case_id, slot) used at
/// write time is required: AAD mismatch fails closed.
pub async fn read_blob(case_id: Uuid, slot: &str, rel: &str) -> Result<Vec<u8>, String> {
    if rel.contains("..") || rel.starts_with('/') {
        return Err("invalid path".into());
    }
    let abs = private_dir().join(rel);
    let raw = tokio::fs::read(&abs).await.map_err(|e| format!("read: {e}"))?;
    if raw.len() < NONCE_LEN + 16 {
        return Err("blob truncated".into());
    }
    let (nonce_bytes, ct) = raw.split_at(NONCE_LEN);
    let key = derive_key(case_id, slot)?;
    let aead = Aes256Gcm::new(AesKey::<Aes256Gcm>::from_slice(&key));
    let nonce = Nonce::from_slice(nonce_bytes);
    let aad = format!("{case_id}|{slot}");
    aead.decrypt(nonce, Payload { msg: ct, aad: aad.as_bytes() })
        .map_err(|e| format!("aead decrypt: {e}"))
}

/// Best-effort overwrite-then-unlink so a later raw-disk-block recovery
/// yields zeros instead of the previous ciphertext. We're conservative:
/// errors are logged by the caller; the row state is the source of
/// truth for "this is now gone", and we tolerate the unlink itself
/// failing.
pub async fn purge_blob(rel: &str) -> Result<(), String> {
    if rel.contains("..") || rel.starts_with('/') {
        return Err("invalid path".into());
    }
    let abs = private_dir().join(rel);
    let Ok(meta) = tokio::fs::metadata(&abs).await else { return Ok(()); };
    let len = meta.len() as usize;
    // Overwrite in place
    if let Ok(mut f) = tokio::fs::OpenOptions::new().write(true).open(&abs).await {
        let zeros = vec![0u8; len.min(64 * 1024)];
        let mut written = 0usize;
        while written < len {
            let want = (len - written).min(zeros.len());
            if f.write_all(&zeros[..want]).await.is_err() { break; }
            written += want;
        }
        let _ = f.flush().await;
        let _ = f.sync_all().await;
    }
    let _ = tokio::fs::remove_file(&abs).await;
    // Best-effort empty-parent cleanup so an old case dir doesn't
    // linger forever.
    if let Some(parent) = Path::new(&abs).parent() {
        let _ = tokio::fs::remove_dir(parent).await;
    }
    Ok(())
}
