//! Wallet-based auth handlers — web + Android + iOS compatible.
use axum::{
    extract::State,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, services::auth, models::ApiResponse};
use crate::api::sessions::{self, RefreshOutcome};
use crate::api::middleware::AuthUser;

#[derive(Debug, Deserialize)]
pub struct NonceRequest { pub address: String }

#[derive(Debug, Serialize)]
pub struct NonceResponse { pub nonce: String, pub message: String }

#[derive(Debug, Deserialize)]
pub struct VerifyRequest {
    pub address: String,
    pub signature: String,
    pub nonce: String,
    /// Optional short human-readable device label ("Stefan's iPhone").
    /// Shown in /me/sessions; truncated to 60 chars server-side.
    #[serde(default)]
    pub device_label: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub needs_email: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct RefreshRequest { pub refresh_token: String }

pub async fn get_nonce(
    State(state): State<AppState>,
    Json(req): Json<NonceRequest>,
) -> AppResult<Json<ApiResponse<NonceResponse>>> {
    let address = req.address.to_lowercase();
    if !is_valid_address(&address) {
        return Err(AppError::Validation("Invalid wallet address".into()));
    }
    let nonce = auth::generate_nonce();
    let message = auth::sign_message(&nonce);
    state.cache.set_nonce(&address, &nonce, Duration::from_secs(600)).await
        .map_err(|e| AppError::Cache(e.to_string()))?;
    Ok(Json(ApiResponse::ok(NonceResponse { nonce, message })))
}

pub async fn verify_signature(
    State(state): State<AppState>,
    Json(req): Json<VerifyRequest>,
) -> AppResult<Json<ApiResponse<TokenResponse>>> {
    let address = req.address.to_lowercase();
    let stored_nonce = state.cache.consume_nonce(&address).await
        .map_err(|e| AppError::Cache(e.to_string()))?
        .ok_or_else(|| AppError::Unauthorised("Nonce not found or expired".into()))?;
    if stored_nonce != req.nonce {
        return Err(AppError::Unauthorised("Nonce mismatch".into()));
    }
    let message = auth::sign_message(&req.nonce);
    let recovered = auth::recover_signer(&message, &req.signature)
        .map_err(|e| AppError::Unauthorised(format!("Signature invalid: {e}")))?;
    if recovered != address {
        return Err(AppError::Unauthorised("Signature does not match address".into()));
    }
    // Upsert user. For first-time wallet logins we generate a placeholder
    // username that the onboarding modal will replace.
    let fallback_username = format!("w_{}", &address[2..10]);
    sqlx::query(
        "INSERT INTO users (wallet_address, username)
         VALUES ($1, $2)
         ON CONFLICT (wallet_address) DO UPDATE SET updated_at = NOW()"
    )
    .bind(&address)
    .bind(&fallback_username)
    .execute(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    let user_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
        .bind(&address)
        .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;

    // Does this user have a verified email? If not -> frontend shows onboarding.
    let needs_email: bool = sqlx::query_scalar(
        "SELECT email IS NULL OR email_verified_at IS NULL FROM users WHERE id = $1"
    )
    .bind(user_id)
    .fetch_one(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    let (access_token, refresh_token) = auth::issue_token_pair(&address, &state.jwt)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Record the refresh-token JTI server-side so subsequent rotations
    // can detect reuse. Best-effort: a DB hiccup here doesn't fail the
    // login; the worst case is we lose reuse-detection for this
    // session (the access-token blacklist still works).
    if let Ok(claims) = auth::verify_refresh_token(&refresh_token, &state.jwt) {
        let label = sanitize_device_label(req.device_label.as_deref());
        let _ = sessions::record_login(
            state.db.pool(), user_id, &claims.jti, label.as_deref(), None,
        ).await;
    }

    Ok(Json(ApiResponse::ok(TokenResponse {
        access_token, refresh_token, token_type: "Bearer".into(),
        needs_email: Some(needs_email),
    })))
}

#[derive(Debug, Deserialize)]
pub struct LogoutRequest {
    /// Optional refresh token so we can blacklist it too. The access
    /// token's JTI comes from the AuthUser extractor.
    #[serde(default)]
    pub refresh_token: Option<String>,
}

/// POST /api/v1/auth/logout
///
/// Properly ends the session server-side: blacklists the current
/// access token's JTI (so it can't be reused for its remaining TTL),
/// blacklists the supplied refresh token if any, and revokes ALL of
/// the user's refresh-token sessions in user_sessions. The client is
/// responsible for clearing its local token copies; this makes a
/// stolen/leaked token useless even if it wasn't cleared everywhere.
pub async fn logout(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<LogoutRequest>,
) -> Result<Response, AppError> {
    // Blacklist this access token for the rest of its lifetime.
    let _ = state.cache.blacklist_token(
        &auth.jti, Duration::from_secs(state.jwt.access_ttl_secs)
    ).await;

    // Blacklist the refresh token (if the client sent it) immediately.
    if let Some(rt) = &req.refresh_token {
        if let Ok(claims) = auth::verify_refresh_token(rt, &state.jwt) {
            let _ = state.cache.blacklist_token(
                &claims.jti, Duration::from_secs(state.jwt.refresh_ttl_secs)
            ).await;
        }
    }

    // Revoke every refresh session this user has (mirrors the
    // "sign out everywhere" path). Best-effort: resolve the user id
    // from the access subject.
    let user_id: Option<Uuid> = if let Some(rest) = auth.address.strip_prefix("email:") {
        Uuid::parse_str(rest).ok()
    } else {
        sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
            .bind(&auth.address)
            .fetch_optional(state.db.pool()).await.ok().flatten()
    };
    if let Some(uid) = user_id {
        let _ = sessions::revoke_all_for_user(
            state.db.pool(), &state.cache, uid, state.jwt.refresh_ttl_secs,
        ).await;
    }

    // Strict no-store so no proxy or browser HTTP cache holds onto a
    // "logout succeeded" 200 — every logout must hit the server so
    // the blacklist + session revocation actually run.
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-cache, must-revalidate, private, max-age=0"));
    headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    let body = Json(ApiResponse::ok("logged_out"));
    Ok((StatusCode::OK, headers, body).into_response())
}

pub async fn refresh_token(
    State(state): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> AppResult<Json<ApiResponse<TokenResponse>>> {
    let claims = auth::verify_refresh_token(&req.refresh_token, &state.jwt)
        .map_err(|e| AppError::Unauthorised(e.to_string()))?;
    if state.cache.is_blacklisted(&claims.jti).await.unwrap_or(false) {
        return Err(AppError::Unauthorised("Token revoked".into()));
    }

    // Mint the new pair first so we have the new JTI to thread through
    // rotation. If rotation fails we discard the new pair and reply 401.
    let (access_token, refresh_token_new) = auth::issue_token_pair(&claims.sub, &state.jwt)
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let new_claims = auth::verify_refresh_token(&refresh_token_new, &state.jwt)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    match sessions::rotate_refresh(state.db.pool(), &claims.jti, &new_claims.jti).await? {
        RefreshOutcome::Ok { .. } => {
            // Blacklist the consumed refresh so concurrent attempts
            // immediately bounce on the access-middleware path too.
            let _ = state.cache.blacklist_token(
                &claims.jti,
                Duration::from_secs(state.jwt.refresh_ttl_secs)
            ).await;
            Ok(Json(ApiResponse::ok(TokenResponse {
                access_token, refresh_token: refresh_token_new,
                token_type: "Bearer".into(), needs_email: None,
            })))
        }
        RefreshOutcome::Reuse { family_id } => {
            // SMOKING GUN: someone (legit or attacker) presented an
            // already-consumed refresh token. We can't tell which side
            // is real, so we revoke the entire family and force a
            // password/wallet re-auth across all of that user's
            // currently-connected sessions.
            let _ = sessions::revoke_family(
                state.db.pool(), &state.cache, family_id, state.jwt.refresh_ttl_secs,
            ).await;
            tracing::warn!(
                family_id = %family_id, sub = %claims.sub,
                "Refresh token reuse detected — revoked entire family"
            );
            Err(AppError::Unauthorised("Session compromised — please log in again".into()))
        }
        RefreshOutcome::Unknown => {
            // Pre-sessions-table legacy token, or a forgery. Reject.
            Err(AppError::Unauthorised("Token revoked".into()))
        }
    }
}

fn is_valid_address(a: &str) -> bool {
    a.starts_with("0x") && a.len() == 42 && a[2..].chars().all(|c| c.is_ascii_hexdigit())
}

pub(crate) fn sanitize_device_label(label: Option<&str>) -> Option<String> {
    let raw = label?.trim();
    if raw.is_empty() { return None; }
    // Drop anything non-printable + truncate at 60 chars. The label
    // surfaces on /me/sessions, never on any public profile.
    let cleaned: String = raw.chars()
        .filter(|c| !c.is_control())
        .take(60)
        .collect();
    if cleaned.is_empty() { None } else { Some(cleaned) }
}
