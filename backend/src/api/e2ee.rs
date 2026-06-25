//! E2EE identity-key storage.
//!
//! The server stores two opaque blobs per user:
//! - `e2ee_public_key`: an ECDH-P256 SPKI-DER public key, base64. Anyone
//!   may fetch it (it has to be discoverable so peers can derive the
//!   conversation key).
//! - `e2ee_encrypted_private_key`: the user's private key sealed with
//!   `AES-GCM(master_key, sk_pkcs8, IV)` where `master_key` is derived
//!   client-side from a deterministic wallet signature. The server
//!   never sees the master key or the plaintext private key.
//!
//! The handlers below are *pure I/O*; all crypto lives in the browser.

use axum::{extract::{Path, State}, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;

async fn caller_user_id(state: &AppState, auth: &AuthUser) -> AppResult<Uuid> {
    if let Some(rest) = auth.address.strip_prefix("email:") {
        return Uuid::parse_str(rest)
            .map_err(|_| AppError::Validation("Invalid user id".into()));
    }
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE wallet_address = $1")
        .bind(&auth.address)
        .fetch_optional(state.db.pool())
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))
}

async fn resolve_user(state: &AppState, address_or_id: &str) -> AppResult<Uuid> {
    // UUID, 0x-wallet, or @username — handled in one place.
    crate::api::conversations::resolve_user(state.db.pool(), address_or_id).await
}

#[derive(Debug, Deserialize)]
pub struct UploadKeysRequest {
    pub public_key: String,
    pub encrypted_private_key: String,
}

#[derive(Debug, Serialize)]
pub struct MyKeysResponse {
    pub public_key: Option<String>,
    pub encrypted_private_key: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PeerPubkeyResponse {
    pub user_id: Uuid,
    pub public_key: Option<String>,
}

/// Plausibility-bound on a base64-encoded P-256 SPKI key (~91 bytes).
const PUBKEY_MAX_LEN: usize = 200;
/// Plausibility-bound on the encrypted PKCS8 private key blob. PKCS8
/// for P-256 is around 138 bytes; AES-GCM adds 16 bytes for the tag
/// and 12 bytes for the IV. Base64-encode → ~250 chars. Round up.
const ENC_SK_MAX_LEN: usize = 600;

pub async fn upload_keys(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<UploadKeysRequest>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    if req.public_key.is_empty() || req.encrypted_private_key.is_empty() {
        return Err(AppError::Validation("Both keys are required".into()));
    }
    if req.public_key.len() > PUBKEY_MAX_LEN || req.encrypted_private_key.len() > ENC_SK_MAX_LEN {
        return Err(AppError::Validation("Key blob too large".into()));
    }
    // We treat the blobs as opaque base64. No structural checks: the
    // server is intentionally blind to the key material.
    if !req.public_key.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' || c == '-' || c == '_') {
        return Err(AppError::Validation("public_key must be base64".into()));
    }
    if !req.encrypted_private_key.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' || c == '-' || c == '_') {
        return Err(AppError::Validation("encrypted_private_key must be base64".into()));
    }

    let me = caller_user_id(&state, &auth).await?;
    sqlx::query(
        "UPDATE users SET e2ee_public_key = $1, e2ee_encrypted_private_key = $2 WHERE id = $3"
    )
    .bind(&req.public_key)
    .bind(&req.encrypted_private_key)
    .bind(me)
    .execute(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok("ok")))
}

pub async fn get_my_keys(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<MyKeysResponse>>> {
    let me = caller_user_id(&state, &auth).await?;
    let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT e2ee_public_key, e2ee_encrypted_private_key FROM users WHERE id = $1"
    )
    .bind(me)
    .fetch_optional(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    let (pk, sk) = row.unwrap_or((None, None));
    Ok(Json(ApiResponse::ok(MyKeysResponse {
        public_key: pk,
        encrypted_private_key: sk,
    })))
}

/// Public-key lookup. Anyone authenticated may fetch any user's public
/// key; the private blob is never exposed by this endpoint.
pub async fn get_peer_pubkey(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(address): Path<String>,
) -> AppResult<Json<ApiResponse<PeerPubkeyResponse>>> {
    let id = resolve_user(&state, &address).await?;
    let pk: Option<String> = sqlx::query_scalar(
        "SELECT e2ee_public_key FROM users WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(state.db.pool())
    .await
    .map_err(AppError::Database)?
    .flatten();

    Ok(Json(ApiResponse::ok(PeerPubkeyResponse { user_id: id, public_key: pk })))
}

// ─── Prekeys (Forward Secrecy phase 1) ──────────────────────────────────
//
// Signal-style key bundle storage. The server stays blind: every field
// is opaque base64, and the signature on the signed prekey is verified
// by the *recipient's peer*, never here. See migration 0033.

/// Cap on a batch of one-time prekeys per upload so a client can't
/// flood the table. 100 is a generous replenish batch.
const MAX_OTP_BATCH: usize = 100;
const SIGNATURE_MAX_LEN: usize = 200;

fn is_b64(s: &str) -> bool {
    !s.is_empty() && s.len() <= PUBKEY_MAX_LEN
        && s.chars().all(|c| c.is_ascii_alphanumeric()
            || c == '+' || c == '/' || c == '=' || c == '-' || c == '_')
}

#[derive(Debug, Deserialize)]
pub struct SignedPrekeyInput {
    pub key_id: i32,
    pub public_key: String,
    pub signature: String,
}

#[derive(Debug, Deserialize)]
pub struct OneTimePrekeyInput {
    pub key_id: i32,
    pub public_key: String,
}

#[derive(Debug, Deserialize)]
pub struct UploadPrekeysRequest {
    /// The ECDSA P-256 signing identity public key (base64 SPKI). Sent
    /// the first time a client provisions prekeys so peers can verify
    /// the signed-prekey signature. Optional on subsequent replenishes.
    pub signing_public_key: Option<String>,
    /// Optional: only present when (re)rotating the signed prekey.
    pub signed_prekey: Option<SignedPrekeyInput>,
    /// Optional: a batch of fresh one-time prekeys to top up the pool.
    #[serde(default)]
    pub one_time_prekeys: Vec<OneTimePrekeyInput>,
}

/// POST /api/v1/me/e2ee/prekeys
/// Idempotent-ish: rotating the signed prekey deactivates the previous
/// active one; one-time prekeys with a duplicate (user, key_id) are
/// skipped via ON CONFLICT so a retried replenish doesn't error.
pub async fn upload_prekeys(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<UploadPrekeysRequest>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    if req.one_time_prekeys.len() > MAX_OTP_BATCH {
        return Err(AppError::Validation("too many one-time prekeys in one batch".into()));
    }
    if let Some(sp) = &req.signed_prekey {
        if !is_b64(&sp.public_key) {
            return Err(AppError::Validation("signed_prekey.public_key must be base64".into()));
        }
        if sp.signature.is_empty() || sp.signature.len() > SIGNATURE_MAX_LEN
            || !sp.signature.chars().all(|c| c.is_ascii_alphanumeric()
                || c == '+' || c == '/' || c == '=' || c == '-' || c == '_') {
            return Err(AppError::Validation("signed_prekey.signature must be base64".into()));
        }
    }
    for otp in &req.one_time_prekeys {
        if !is_b64(&otp.public_key) {
            return Err(AppError::Validation("one_time_prekey.public_key must be base64".into()));
        }
    }

    if let Some(spk) = &req.signing_public_key {
        if !is_b64(spk) {
            return Err(AppError::Validation("signing_public_key must be base64".into()));
        }
    }

    let me = caller_user_id(&state, &auth).await?;
    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;

    if let Some(spk) = &req.signing_public_key {
        sqlx::query("UPDATE users SET e2ee_signing_public_key = $1 WHERE id = $2")
            .bind(spk).bind(me)
            .execute(&mut *tx).await.map_err(AppError::Database)?;
    }

    if let Some(sp) = &req.signed_prekey {
        // Deactivate the current active signed prekey, then insert the
        // new one as active. The partial unique index guarantees there
        // is never more than one active row per user.
        sqlx::query("UPDATE signed_prekeys SET active = FALSE WHERE user_id = $1 AND active = TRUE")
            .bind(me)
            .execute(&mut *tx).await.map_err(AppError::Database)?;
        sqlx::query(
            "INSERT INTO signed_prekeys (user_id, key_id, public_key, signature, active)
             VALUES ($1, $2, $3, $4, TRUE)
             ON CONFLICT (user_id, key_id) DO UPDATE
               SET public_key = EXCLUDED.public_key,
                   signature  = EXCLUDED.signature,
                   active     = TRUE"
        )
        .bind(me).bind(sp.key_id).bind(&sp.public_key).bind(&sp.signature)
        .execute(&mut *tx).await.map_err(AppError::Database)?;
    }

    for otp in &req.one_time_prekeys {
        sqlx::query(
            "INSERT INTO one_time_prekeys (user_id, key_id, public_key)
             VALUES ($1, $2, $3)
             ON CONFLICT (user_id, key_id) DO NOTHING"
        )
        .bind(me).bind(otp.key_id).bind(&otp.public_key)
        .execute(&mut *tx).await.map_err(AppError::Database)?;
    }

    tx.commit().await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok("ok")))
}

#[derive(Debug, Serialize)]
pub struct PrekeyCountResponse {
    pub one_time_prekeys_available: i64,
    pub has_signed_prekey: bool,
}

/// GET /api/v1/me/e2ee/prekeys/count — lets the client decide when to
/// replenish (e.g. top up when available drops below ~20).
pub async fn prekey_count(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<PrekeyCountResponse>>> {
    let me = caller_user_id(&state, &auth).await?;
    let available: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM one_time_prekeys WHERE user_id = $1 AND used_at IS NULL"
    )
    .bind(me)
    .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
    let has_signed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM signed_prekeys WHERE user_id = $1 AND active = TRUE)"
    )
    .bind(me)
    .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(PrekeyCountResponse {
        one_time_prekeys_available: available,
        has_signed_prekey: has_signed,
    })))
}

#[derive(Debug, Serialize)]
pub struct BundleSignedPrekey {
    pub key_id: i32,
    pub public_key: String,
    pub signature: String,
}

#[derive(Debug, Serialize)]
pub struct BundleOneTimePrekey {
    pub key_id: i32,
    pub public_key: String,
}

#[derive(Debug, Serialize)]
pub struct PrekeyBundleResponse {
    pub user_id: Uuid,
    pub identity_key: Option<String>,
    /// ECDSA P-256 public key used to verify `signed_prekey.signature`.
    pub signing_identity_key: Option<String>,
    pub signed_prekey: Option<BundleSignedPrekey>,
    /// One claimed one-time prekey, or null if the recipient's pool is
    /// exhausted. X3DH degrades safely without an OTP (one less DH),
    /// so a null here is usable, just slightly weaker.
    pub one_time_prekey: Option<BundleOneTimePrekey>,
}

/// GET /api/v1/users/:address/e2ee/bundle
/// Returns a recipient's key bundle for establishing a forward-secret
/// session, atomically consuming one one-time prekey. The OTP claim
/// uses FOR UPDATE SKIP LOCKED so two concurrent senders never get the
/// same key.
pub async fn get_prekey_bundle(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(address): Path<String>,
) -> AppResult<Json<ApiResponse<PrekeyBundleResponse>>> {
    let id = resolve_user(&state, &address).await?;

    let id_keys: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT e2ee_public_key, e2ee_signing_public_key FROM users WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (identity_key, signing_identity_key) = id_keys.unwrap_or((None, None));

    let signed: Option<(i32, String, String)> = sqlx::query_as(
        "SELECT key_id, public_key, signature FROM signed_prekeys
          WHERE user_id = $1 AND active = TRUE
          LIMIT 1"
    )
    .bind(id)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;

    // Atomically claim one unused OTP. SKIP LOCKED so concurrent
    // bundle fetches for the same user don't collide on the same row.
    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;
    let otp: Option<(Uuid, i32, String)> = sqlx::query_as(
        "SELECT id, key_id, public_key FROM one_time_prekeys
          WHERE user_id = $1 AND used_at IS NULL
          ORDER BY created_at ASC
          LIMIT 1
          FOR UPDATE SKIP LOCKED"
    )
    .bind(id)
    .fetch_optional(&mut *tx).await.map_err(AppError::Database)?;
    if let Some((otp_id, _, _)) = &otp {
        sqlx::query("UPDATE one_time_prekeys SET used_at = NOW() WHERE id = $1")
            .bind(otp_id)
            .execute(&mut *tx).await.map_err(AppError::Database)?;
    }
    tx.commit().await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(PrekeyBundleResponse {
        user_id: id,
        identity_key,
        signing_identity_key,
        signed_prekey: signed.map(|(key_id, public_key, signature)| BundleSignedPrekey {
            key_id, public_key, signature,
        }),
        one_time_prekey: otp.map(|(_, key_id, public_key)| BundleOneTimePrekey {
            key_id, public_key,
        }),
    })))
}
