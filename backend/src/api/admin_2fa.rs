//! Mandatory two-factor authentication for the admin surface.
//!
//! The admin secret alone no longer opens any `/api/v1/admin/*` endpoint:
//! every request must also carry an `X-Admin-Session` token, and a session
//! is only issued against the secret plus a current authenticator code (or
//! a one-time recovery code). Until an authenticator is enrolled the only
//! admin endpoints that answer are the enrolment ones here, so the first
//! thing a fresh deploy asks its operator to do is scan the QR code.
//! Enforcement lives in `middleware::abuse_guard` (one place, all routes).
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::admin_mod::check_admin_secret;
use crate::services::totp;

const ACCOUNT: &str = "admin";
/// Admin session lifetime. Short: the panel re-asks for a code after it.
pub const SESSION_TTL_SECS: u64 = 30 * 60;

fn session_key(token: &str) -> String { format!("admsess:{token}") }

/// Is this admin session token valid? (Stored with the cache's generic
/// "key exists until TTL" primitive.)
pub async fn session_valid(state: &AppState, token: &str) -> bool {
    !token.is_empty() && token.len() <= 128
        && state.cache.is_blacklisted(&session_key(token)).await.unwrap_or(false)
}

pub async fn enrolled(state: &AppState) -> bool {
    sqlx::query_scalar::<_, Option<chrono::DateTime<chrono::Utc>>>(
        "SELECT enabled_at FROM admin_totp WHERE id = 1"
    ).fetch_optional(state.db.pool()).await.ok().flatten().flatten().is_some()
}

async fn new_session(state: &AppState) -> AppResult<String> {
    let token = format!("{}{}", crate::services::auth::generate_nonce(), crate::services::auth::generate_nonce());
    state.cache.blacklist_token(&session_key(&token), std::time::Duration::from_secs(SESSION_TTL_SECS)).await
        .map_err(|e| AppError::Cache(e.to_string()))?;
    Ok(token)
}

async fn verify_code(state: &AppState, code: &str) -> AppResult<bool> {
    let row: Option<(Option<String>, Vec<String>)> = sqlx::query_as(
        "SELECT secret_enc, recovery_hashes FROM admin_totp WHERE id = 1"
    ).fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let Some((Some(enc), recovery)) = row else { return Ok(false); };
    if totp::verify_enc(&enc, ACCOUNT, code) { return Ok(true); }
    let h = totp::hash_code(code);
    if recovery.iter().any(|r| r == &h) {
        sqlx::query("UPDATE admin_totp SET recovery_hashes = array_remove(recovery_hashes, $1) WHERE id = 1")
            .bind(&h).execute(state.db.pool()).await.map_err(AppError::Database)?;
        return Ok(true);
    }
    Ok(false)
}

#[derive(Deserialize)]
pub struct SecretRequest { pub secret: String }
#[derive(Deserialize)]
pub struct SecretCodeRequest { pub secret: String, pub code: String }

#[derive(Serialize)]
pub struct StatusResponse { pub enrolled: bool }

pub async fn status(State(state): State<AppState>, Json(req): Json<SecretRequest>) -> AppResult<Json<ApiResponse<StatusResponse>>> {
    check_admin_secret(&req.secret)?;
    Ok(Json(ApiResponse::ok(StatusResponse { enrolled: enrolled(&state).await })))
}

#[derive(Serialize)]
pub struct SetupResponse { pub secret: String, pub otpauth_url: String, pub qr_png_base64: String }

/// Start enrolment. Only while no authenticator is active: replacing an
/// enrolled one needs a database-level reset by the operator.
pub async fn setup(State(state): State<AppState>, Json(req): Json<SecretRequest>) -> AppResult<Json<ApiResponse<SetupResponse>>> {
    check_admin_secret(&req.secret)?;
    if enrolled(&state).await {
        return Err(AppError::Validation("Admin 2FA is already enrolled".into()));
    }
    let b32 = totp::generate_secret_b32();
    let otpauth_url = totp::otpauth_url(&b32, ACCOUNT).map_err(AppError::Internal)?;
    let qr = totp::qr_png_base64(&b32, ACCOUNT).map_err(AppError::Internal)?;
    let enc = totp::seal(&b32).map_err(AppError::Internal)?;
    sqlx::query("UPDATE admin_totp SET pending_enc = $1 WHERE id = 1")
        .bind(&enc).execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(SetupResponse { secret: b32, otpauth_url, qr_png_base64: qr })))
}

#[derive(Serialize)]
pub struct ActivateResponse { pub recovery_codes: Vec<String>, pub session: String, pub expires_in: u64 }

pub async fn activate(State(state): State<AppState>, Json(req): Json<SecretCodeRequest>) -> AppResult<Json<ApiResponse<ActivateResponse>>> {
    check_admin_secret(&req.secret)?;
    if enrolled(&state).await {
        return Err(AppError::Validation("Admin 2FA is already enrolled".into()));
    }
    let pending: Option<Option<String>> = sqlx::query_scalar("SELECT pending_enc FROM admin_totp WHERE id = 1")
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let Some(Some(pending_enc)) = pending else {
        return Err(AppError::Validation("No pending setup; call setup first".into()));
    };
    if !totp::verify_enc(&pending_enc, ACCOUNT, &req.code) {
        return Err(AppError::Unauthorised("Code is not valid".into()));
    }
    let (plain, hashes) = totp::generate_recovery_codes();
    sqlx::query("UPDATE admin_totp SET secret_enc = $1, pending_enc = NULL, enabled_at = NOW(), recovery_hashes = $2 WHERE id = 1")
        .bind(&pending_enc).bind(&hashes).execute(state.db.pool()).await.map_err(AppError::Database)?;
    tracing::warn!("admin 2FA enrolled");
    let session = new_session(&state).await?;
    Ok(Json(ApiResponse::ok(ActivateResponse { recovery_codes: plain, session, expires_in: SESSION_TTL_SECS })))
}

#[derive(Serialize)]
pub struct SessionResponse { pub session: String, pub expires_in: u64 }

/// Exchange secret + code for a 30-minute admin session.
pub async fn session(State(state): State<AppState>, Json(req): Json<SecretCodeRequest>) -> AppResult<Json<ApiResponse<SessionResponse>>> {
    check_admin_secret(&req.secret)?;
    if !enrolled(&state).await {
        return Err(AppError::Unauthorised("ADMIN_2FA_SETUP_REQUIRED: enrol an authenticator first".into()));
    }
    if !verify_code(&state, &req.code).await? {
        tracing::warn!("admin 2FA: wrong code");
        return Err(AppError::Unauthorised("Code is not valid".into()));
    }
    let session = new_session(&state).await?;
    Ok(Json(ApiResponse::ok(SessionResponse { session, expires_in: SESSION_TTL_SECS })))
}
