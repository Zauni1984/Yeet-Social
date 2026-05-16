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
