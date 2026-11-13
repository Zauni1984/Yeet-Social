//! Two-factor authentication endpoints (TOTP authenticator apps).
//!
//! Enrolment is two steps: `setup` stores a *pending* secret and returns the
//! QR / otpauth URL; `activate` only turns 2FA on once the user proves they
//! scanned it by entering a current code, and then hands out one-time
//! recovery codes. Login with 2FA on does not return tokens from the
//! password step; it returns a short-lived challenge that `/auth/mfa`
//! exchanges for tokens after a valid code or recovery code. The TOTP secret
//! lives encrypted at rest (see services::totp).
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;
use crate::services::totp;

async fn uid(state: &AppState, auth: &AuthUser) -> AppResult<Uuid> {
    crate::api::users::resolve_user_id(state, &auth.address).await
}

/// Account name shown in the authenticator (and bound into the secret build).
/// Stable per user so a re-build verifies the same codes.
fn account_name(uid: Uuid) -> String { uid.to_string() }

/// Whether 2FA is active for this user.
pub async fn is_enabled(pool: &sqlx::PgPool, user_id: Uuid) -> bool {
    sqlx::query_scalar::<_, Option<chrono::DateTime<chrono::Utc>>>(
        "SELECT totp_enabled_at FROM users WHERE id = $1"
    ).bind(user_id).fetch_optional(pool).await.ok().flatten().flatten().is_some()
}

/// Verify a second factor for `user_id`: a current TOTP code, or a one-time
/// recovery code (which is then consumed). Used at login and for step-up.
pub async fn verify_second_factor(state: &AppState, user_id: Uuid, code: &str) -> AppResult<bool> {
    let row: Option<(Option<String>, Vec<String>)> = sqlx::query_as(
        "SELECT totp_secret_enc, totp_recovery_hashes FROM users WHERE id = $1"
    ).bind(user_id).fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let Some((Some(secret_enc), recovery)) = row else { return Ok(false); };
    if totp::verify_enc(&secret_enc, &account_name(user_id), code) {
        return Ok(true);
    }
    // Recovery code path: constant-ish work, remove on match.
    let h = totp::hash_code(code);
    if recovery.iter().any(|r| r == &h) {
        sqlx::query("UPDATE users SET totp_recovery_hashes = array_remove(totp_recovery_hashes, $2) WHERE id = $1")
            .bind(user_id).bind(&h).execute(state.db.pool()).await.map_err(AppError::Database)?;
        return Ok(true);
    }
    Ok(false)
}

#[derive(Serialize)]
pub struct StatusResponse { pub enabled: bool, pub recovery_remaining: i64 }

pub async fn status(State(state): State<AppState>, auth: AuthUser) -> AppResult<Json<ApiResponse<StatusResponse>>> {
    let me = uid(&state, &auth).await?;
    let row: Option<(Option<chrono::DateTime<chrono::Utc>>, Vec<String>)> = sqlx::query_as(
        "SELECT totp_enabled_at, totp_recovery_hashes FROM users WHERE id = $1"
    ).bind(me).fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (enabled_at, recovery) = row.unwrap_or((None, Vec::new()));
    Ok(Json(ApiResponse::ok(StatusResponse { enabled: enabled_at.is_some(), recovery_remaining: recovery.len() as i64 })))
}

#[derive(Serialize)]
pub struct SetupResponse { pub secret: String, pub otpauth_url: String, pub qr_png_base64: String }

/// Begin enrolment: generate a pending secret and return the QR + URL. Does
/// not enable 2FA. Refused if 2FA is already on (disable first).
pub async fn setup(State(state): State<AppState>, auth: AuthUser) -> AppResult<Json<ApiResponse<SetupResponse>>> {
    let me = uid(&state, &auth).await?;
    crate::api::middleware::limit(&state, "2fa_setup", &auth.address, 600, 5, 3600, 20).await?;
    if is_enabled(state.db.pool(), me).await {
        return Err(AppError::Validation("2FA is already enabled; disable it first".into()));
    }
    let b32 = totp::generate_secret_b32();
    let acct = account_name(me);
    let otpauth_url = totp::otpauth_url(&b32, &acct).map_err(AppError::Internal)?;
    let qr = totp::qr_png_base64(&b32, &acct).map_err(AppError::Internal)?;
    let enc = totp::seal(&b32).map_err(AppError::Internal)?;
    sqlx::query("UPDATE users SET totp_pending_enc = $2 WHERE id = $1")
        .bind(me).bind(&enc).execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(SetupResponse { secret: b32, otpauth_url, qr_png_base64: qr })))
}

#[derive(Deserialize)]
pub struct CodeRequest { pub code: String }

#[derive(Serialize)]
pub struct ActivateResponse { pub recovery_codes: Vec<String> }

/// Finish enrolment: verify a code against the pending secret, enable 2FA and
/// return one-time recovery codes (shown once).
pub async fn activate(State(state): State<AppState>, auth: AuthUser, Json(req): Json<CodeRequest>) -> AppResult<Json<ApiResponse<ActivateResponse>>> {
    let me = uid(&state, &auth).await?;
    crate::api::middleware::limit(&state, "2fa_activate", &auth.address, 600, 10, 3600, 40).await?;
    let pending: Option<Option<String>> = sqlx::query_scalar(
        "SELECT totp_pending_enc FROM users WHERE id = $1"
    ).bind(me).fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let Some(Some(pending_enc)) = pending else {
        return Err(AppError::Validation("No pending 2FA setup; call setup first".into()));
    };
    if !totp::verify_enc(&pending_enc, &account_name(me), &req.code) {
        return Err(AppError::Unauthorised("Code is not valid".into()));
    }
    let (plain, hashes) = totp::generate_recovery_codes();
    sqlx::query(
        "UPDATE users SET totp_secret_enc = $2, totp_pending_enc = NULL,
                          totp_enabled_at = NOW(), totp_recovery_hashes = $3
           WHERE id = $1"
    ).bind(me).bind(&pending_enc).bind(&hashes)
     .execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(ActivateResponse { recovery_codes: plain })))
}

/// Turn 2FA off. Requires a current code or a recovery code so a hijacked
/// session cannot silently remove the second factor.
pub async fn disable(State(state): State<AppState>, auth: AuthUser, Json(req): Json<CodeRequest>) -> AppResult<Json<ApiResponse<&'static str>>> {
    let me = uid(&state, &auth).await?;
    crate::api::middleware::limit(&state, "2fa_disable", &auth.address, 600, 10, 3600, 40).await?;
    if !is_enabled(state.db.pool(), me).await {
        return Ok(Json(ApiResponse::ok("ok")));
    }
    if !verify_second_factor(&state, me, &req.code).await? {
        return Err(AppError::Unauthorised("Code is not valid".into()));
    }
    sqlx::query(
        "UPDATE users SET totp_secret_enc = NULL, totp_pending_enc = NULL,
                          totp_enabled_at = NULL, totp_recovery_hashes = '{}'
           WHERE id = $1"
    ).bind(me).execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok("ok")))
}
