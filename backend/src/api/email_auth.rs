//! Email-based authentication + verification (DSGVO double-opt-in).
use axum::{extract::State, Json};
use chrono::{Duration as ChronoDuration, Utc};
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, api::middleware::AuthUser, models::ApiResponse, services::{auth, email as email_svc}};

#[derive(Debug, Deserialize)]
pub struct EmailRegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: Option<String>,
    pub consent: Option<bool>,
    /// Client-derived wallet address (lower-case 0x...). Optional, since
    /// older clients do not send it; when present we persist it so the
    /// account is reachable for tipping / paper-wallet redemption /
    /// follow-from-wallet flows without a separate link-wallet step.
    pub wallet_address: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EmailLoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub access_token:   String,
    pub refresh_token:  String,
    pub token_type:     String,
    pub username:       String,
    pub email_verified: bool,
}

#[derive(Debug, Deserialize)]
pub struct VerifyEmailRequest { pub token: String }

#[derive(Debug, Deserialize)]
pub struct LinkEmailRequest {
    pub email:   String,
    pub consent: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct SimpleOk { pub ok: bool }

/// Legacy salted-SHA-256 hash (pre-Argon2 accounts). Kept ONLY to verify — and
/// then upgrade — old password hashes on the user's next login. Never used to
/// store a new password.
fn legacy_sha256(password: &str, salt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{}{}", password, salt));
    format!("{:x}", hasher.finalize())
}

/// Constant-time byte comparison (avoids leaking match progress via timing on
/// the legacy hex-digest path; Argon2's own verify is already constant-time).
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) { diff |= x ^ y; }
    diff == 0
}

/// Hash a new password with Argon2id. Returns a self-contained PHC string
/// (algorithm, parameters and a random per-password salt are all embedded), so
/// the separate `password_salt` column is not needed for new accounts.
fn hash_password_argon2(password: &str) -> AppResult<String> {
    use argon2::{Argon2, PasswordHasher, password_hash::{SaltString, rand_core::OsRng}};
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(format!("password hashing failed: {e}")))
}

/// Verify a password against the stored credential, transparently supporting
/// both new Argon2 PHC strings and legacy salted-SHA-256 hashes.
///
/// Returns `(ok, needs_upgrade)`: `ok` is whether the password matched;
/// `needs_upgrade` is true only when the match came from a legacy hash, i.e. the
/// caller should re-hash with Argon2 and persist it.
fn verify_password(password: &str, stored_hash: &str, salt: &str) -> (bool, bool) {
    if stored_hash.starts_with("$argon2") {
        use argon2::{Argon2, PasswordVerifier, password_hash::PasswordHash};
        let ok = PasswordHash::new(stored_hash)
            .map(|ph| Argon2::default().verify_password(password.as_bytes(), &ph).is_ok())
            .unwrap_or(false);
        (ok, false)
    } else {
        let ok = ct_eq(legacy_sha256(password, salt).as_bytes(), stored_hash.as_bytes());
        (ok, ok) // a legacy match must be upgraded to Argon2
    }
}

fn gen_token() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(48)
        .map(char::from)
        .collect()
}

async fn issue_and_send_verification(
    state: &AppState,
    user_id: Uuid,
    email: &str,
) -> AppResult<()> {
    let token = gen_token();
    let expires_at = Utc::now() + ChronoDuration::hours(24);

    // Clear any previous pending tokens for this user, then insert a fresh one.
    sqlx::query("DELETE FROM email_verification_tokens WHERE user_id = $1")
        .bind(user_id)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;

    sqlx::query(
        "INSERT INTO email_verification_tokens (token, user_id, email, expires_at)
         VALUES ($1, $2, $3, $4)"
    )
    .bind(&token).bind(user_id).bind(email).bind(expires_at)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;

    if let Some(cfg) = state.email.as_ref() {
        if let Err(e) = email_svc::send_verification_email(cfg, email, &token).await {
            tracing::warn!("SMTP send failed: {e}");
        }
    } else {
        tracing::warn!("SMTP not configured; verification token created but no email sent: {}", token);
    }
    Ok(())
}

pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<EmailRegisterRequest>,
) -> AppResult<Json<ApiResponse<TokenResponse>>> {
    if req.email.is_empty() || !req.email.contains('@') {
        return Err(AppError::Validation("Invalid email address".into()));
    }
    if req.password.len() < 8 {
        return Err(AppError::Validation("Password must be at least 8 characters".into()));
    }
    if req.consent != Some(true) {
        return Err(AppError::Validation("Consent required (DSGVO)".into()));
    }

    let email_lower = req.email.to_lowercase();

    let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(&email_lower)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    if exists.is_some() {
        return Err(AppError::Validation("Email already registered".into()));
    }

    let hash = hash_password_argon2(&req.password)?;
    // Argon2 PHC strings embed their own salt; the legacy per-row salt column is
    // stored empty for new accounts (kept only for un-upgraded legacy hashes).
    let salt = String::new();
    let username_base = email_lower.split('@').next().unwrap_or("user")
        .chars().filter(|c| c.is_alphanumeric() || *c == '_').take(20).collect::<String>();
    let username = unique_username(&state, &username_base).await?;

    // Non-custodial model (docs/mica/05): email accounts are created WITHOUT
    // a wallet. The platform no longer generates or accepts a wallet at
    // signup — a payout wallet is added later, and only via the
    // signature-verified link-wallet flow (link_wallet_verify). Any
    // wallet_address in the request is ignored on purpose.
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, password_salt, username, display_name)
         VALUES ($1, $2, $3, $4, $5) RETURNING id"
    )
    .bind(&email_lower).bind(&hash).bind(&salt).bind(&username)
    .bind(req.display_name.unwrap_or_else(|| username.clone()))
    .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;

    issue_and_send_verification(&state, user_id, &email_lower).await?;

    let subject = format!("email:{}", user_id);
    let (access_token, refresh_token) = auth::issue_token_pair(&subject, &state.jwt)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    record_session_best_effort(&state, user_id, &refresh_token).await;

    Ok(Json(ApiResponse::ok(TokenResponse {
        access_token, refresh_token,
        token_type: "Bearer".into(),
        username,
        email_verified: false,
    })))
}

pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<EmailLoginRequest>,
) -> AppResult<Json<ApiResponse<TokenResponse>>> {
    if req.email.is_empty() || req.password.is_empty() {
        return Err(AppError::Validation("Email and password required".into()));
    }

    let row = sqlx::query_as::<_, (Uuid, String, String, String, Option<chrono::DateTime<Utc>>)>(
        "SELECT id, password_hash, password_salt, COALESCE(username, 'user'), email_verified_at
         FROM users WHERE email = $1"
    )
    .bind(req.email.to_lowercase())
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;

    let (user_id, stored_hash, salt, username, verified_at) = row
        .ok_or_else(|| AppError::Unauthorised("Invalid email or password".into()))?;

    let (ok, needs_upgrade) = verify_password(&req.password, &stored_hash, &salt);
    if !ok {
        return Err(AppError::Unauthorised("Invalid email or password".into()));
    }
    // Transparently migrate a legacy salted-SHA-256 hash to Argon2 now that we
    // hold the plaintext. Best-effort: a failed rehash must not block login.
    if needs_upgrade {
        if let Ok(new_hash) = hash_password_argon2(&req.password) {
            let _ = sqlx::query("UPDATE users SET password_hash = $1, password_salt = '' WHERE id = $2")
                .bind(&new_hash).bind(user_id)
                .execute(state.db.pool()).await;
        }
    }

    let subject = format!("email:{}", user_id);
    let (access_token, refresh_token) = auth::issue_token_pair(&subject, &state.jwt)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    record_session_best_effort(&state, user_id, &refresh_token).await;

    Ok(Json(ApiResponse::ok(TokenResponse {
        access_token, refresh_token,
        token_type: "Bearer".into(),
        username,
        email_verified: verified_at.is_some(),
    })))
}

/// Decode the freshly-issued refresh token's JTI and insert a session
/// row so /me/sessions surfaces this login and refresh-rotation can
/// detect reuse. Failure is logged but never blocks the auth flow —
/// the access-token blacklist still protects active sessions.
async fn record_session_best_effort(state: &AppState, user_id: Uuid, refresh_token: &str) {
    if let Ok(claims) = auth::verify_refresh_token(refresh_token, &state.jwt) {
        if let Err(e) = crate::api::sessions::record_login(
            state.db.pool(), user_id, &claims.jti, None, None,
        ).await {
            tracing::warn!(error = %e, "Failed to record session row");
        }
    }
}

/// Core verification: validate + consume the token, mark the user verified.
/// Shared by the JSON POST and the GET link handlers.
async fn consume_verification_token(state: &AppState, token: &str) -> AppResult<()> {
    if token.len() < 16 {
        return Err(AppError::Validation("Invalid token".into()));
    }

    let row: Option<(Uuid, String, chrono::DateTime<Utc>)> = sqlx::query_as(
        "SELECT user_id, email, expires_at FROM email_verification_tokens WHERE token = $1"
    )
    .bind(token)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;

    let (user_id, email, expires_at) = row
        .ok_or_else(|| AppError::NotFound("Verification link invalid or already used".into()))?;

    if expires_at < Utc::now() {
        sqlx::query("DELETE FROM email_verification_tokens WHERE token = $1")
            .bind(token).execute(state.db.pool()).await.map_err(AppError::Database)?;
        return Err(AppError::Validation("Verification link expired. Request a new one.".into()));
    }

    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;
    sqlx::query(
        "UPDATE users SET email = $2, email_verified_at = NOW(), email_pending = NULL
         WHERE id = $1"
    )
    .bind(user_id).bind(&email)
    .execute(&mut *tx).await.map_err(AppError::Database)?;
    sqlx::query("DELETE FROM email_verification_tokens WHERE user_id = $1")
        .bind(user_id).execute(&mut *tx).await.map_err(AppError::Database)?;
    tx.commit().await.map_err(AppError::Database)?;

    // Double-opt-in just completed — if KYC is also done, pay the signup bonus.
    // Best-effort: a bonus failure must not fail email verification.
    let _ = crate::services::tokens::maybe_grant_registration_bonus(&state.db, user_id).await;
    Ok(())
}

/// POST /api/v1/auth/email-verify — JSON path (used by the in-app SPA flow).
pub async fn verify_email(
    State(state): State<AppState>,
    Json(req): Json<VerifyEmailRequest>,
) -> AppResult<Json<ApiResponse<SimpleOk>>> {
    consume_verification_token(&state, &req.token).await?;
    Ok(Json(ApiResponse::ok(SimpleOk { ok: true })))
}

#[derive(Debug, Deserialize)]
pub struct VerifyLinkQuery { pub token: Option<String> }

/// GET /api/v1/auth/email-verify?token=... — the link clicked from the email.
///
/// Verifies server-side (no dependency on the SPA loading or running JS —
/// which is why the previous client-only flow appeared broken) and returns a
/// small self-contained confirmation page with a button back to the app.
pub async fn verify_email_link(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<VerifyLinkQuery>,
) -> axum::response::Response {
    use axum::http::{header, HeaderValue, StatusCode};
    use axum::response::IntoResponse;

    let base = std::env::var("PUBLIC_BASE_URL").unwrap_or_else(|_| "https://justyeet.it".into());
    let base = base.trim_end_matches('/').to_string();
    let token = q.token.unwrap_or_default();

    let (status, ok, heading, body): (StatusCode, bool, &str, String) =
        match consume_verification_token(&state, &token).await {
        Ok(()) => (StatusCode::OK, true, "E-Mail bestätigt ✓",
            "Dein Konto ist jetzt verifiziert. Du kannst zu YEET zurückkehren.".to_string()),
        Err(AppError::Validation(m)) | Err(AppError::NotFound(m)) =>
            (StatusCode::BAD_REQUEST, false, "Bestätigung fehlgeschlagen", m),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, false, "Bestätigung fehlgeschlagen",
            "Es ist ein Fehler aufgetreten. Bitte fordere einen neuen Link an.".to_string()),
    };

    let color = if ok { "#c6f135" } else { "#ff6b6b" };
    let html = format!(
        r#"<!DOCTYPE html><html lang="de"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>YEET — E-Mail-Bestätigung</title></head>
<body style="font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;background:#0a0a0a;color:#fff;margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;padding:24px">
<div style="max-width:460px;width:100%;background:#16181c;border:1px solid #2a2a2a;border-radius:16px;padding:32px;text-align:center">
<h1 style="color:{color};margin:0 0 12px;font-size:22px">{heading}</h1>
<p style="color:#e0e0e0;line-height:1.6;font-size:15px;margin:0 0 24px">{body}</p>
<a href="{base}/" style="display:inline-block;background:#c6f135;color:#000;padding:12px 28px;border-radius:24px;text-decoration:none;font-weight:700">Zu YEET Social</a>
</div></body></html>"#,
        color = color, heading = heading, body = body, base = base
    );

    let mut resp = (status, axum::response::Html(html)).into_response();
    resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

type PendingEmailRow = (Option<String>, Option<String>, Option<chrono::DateTime<Utc>>);

pub async fn resend_verification(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<SimpleOk>>> {
    let user_id = resolve_user_id(&state, &auth.address).await?;
    let row: Option<PendingEmailRow> = sqlx::query_as(
        "SELECT email, email_pending, email_verified_at FROM users WHERE id = $1"
    )
    .bind(user_id)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (email, pending, verified_at) = row.ok_or_else(|| AppError::NotFound("User not found".into()))?;

    if verified_at.is_some() && pending.is_none() {
        return Err(AppError::Validation("Email already verified".into()));
    }
    let target = pending.or(email)
        .ok_or_else(|| AppError::Validation("No email on file".into()))?;
    issue_and_send_verification(&state, user_id, &target).await?;
    Ok(Json(ApiResponse::ok(SimpleOk { ok: true })))
}

pub async fn link_email(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<LinkEmailRequest>,
) -> AppResult<Json<ApiResponse<SimpleOk>>> {
    if !req.email.contains('@') {
        return Err(AppError::Validation("Invalid email address".into()));
    }
    if req.consent != Some(true) {
        return Err(AppError::Validation("Consent required (DSGVO)".into()));
    }
    let email_lower = req.email.to_lowercase();
    let user_id = resolve_user_id(&state, &auth.address).await?;

    let taken: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM users WHERE email = $1 AND id <> $2"
    )
    .bind(&email_lower).bind(user_id)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    if taken.is_some() {
        return Err(AppError::Validation("Email already registered to another account".into()));
    }

    // Store as pending email until verified.
    sqlx::query("UPDATE users SET email_pending = $2 WHERE id = $1")
        .bind(user_id).bind(&email_lower)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;

    issue_and_send_verification(&state, user_id, &email_lower).await?;
    Ok(Json(ApiResponse::ok(SimpleOk { ok: true })))
}

async fn resolve_user_id(state: &AppState, auth_address: &str) -> AppResult<Uuid> {
    if let Some(uuid_str) = auth_address.strip_prefix("email:") {
        return uuid_str.parse::<Uuid>().map_err(|_| AppError::NotFound("Invalid user ID".into()));
    }
    sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
        .bind(auth_address)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))
}

// ---- Wallet linking for email users ----

#[derive(Debug, Deserialize)]
pub struct LinkWalletNonceRequest { pub address: String }

#[derive(Debug, Serialize)]
pub struct LinkWalletNonceResponse { pub nonce: String, pub message: String }

#[derive(Debug, Deserialize)]
pub struct LinkWalletVerifyRequest {
    pub address: String,
    pub signature: String,
    pub nonce: String,
}

pub async fn link_wallet_nonce(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<LinkWalletNonceRequest>,
) -> AppResult<Json<ApiResponse<LinkWalletNonceResponse>>> {
    let _ = resolve_user_id(&state, &auth.address).await?;
    let address = req.address.to_lowercase();
    if !address.starts_with("0x") || address.len() != 42 {
        return Err(AppError::Validation("Invalid wallet address".into()));
    }
    let nonce = auth::generate_nonce();
    let message = auth::sign_message(&nonce);
    state.cache.set_nonce(&address, &nonce, Duration::from_secs(600)).await
        .map_err(|e| AppError::Cache(e.to_string()))?;
    Ok(Json(ApiResponse::ok(LinkWalletNonceResponse { nonce, message })))
}

pub async fn link_wallet_verify(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<LinkWalletVerifyRequest>,
) -> AppResult<Json<ApiResponse<SimpleOk>>> {
    let user_id = resolve_user_id(&state, &auth.address).await?;
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

    let taken: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM users WHERE wallet_address = $1 AND id <> $2"
    )
    .bind(&address).bind(user_id)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    if taken.is_some() {
        return Err(AppError::Validation("Wallet already linked to another account".into()));
    }

    sqlx::query("UPDATE users SET wallet_address = $2, updated_at = NOW() WHERE id = $1")
        .bind(user_id).bind(&address)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(SimpleOk { ok: true })))
}

async fn unique_username(state: &AppState, base: &str) -> AppResult<String> {
    let base = if base.is_empty() { "user".to_string() } else { base.to_string() };
    for i in 0..20 {
        let candidate = if i == 0 { base.clone() } else { format!("{}{}", base, i) };
        let taken: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
            .bind(&candidate)
            .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
        if taken.is_none() { return Ok(candidate); }
    }
    Ok(format!("{}-{}", base, Uuid::new_v4().simple().to_string().chars().take(6).collect::<String>()))
}

#[cfg(test)]
mod password_tests {
    use super::*;

    #[test]
    fn argon2_roundtrip_and_scheme() {
        let phc = hash_password_argon2("correct horse battery staple").unwrap();
        assert!(phc.starts_with("$argon2"), "expected PHC string, got {phc}");
        // Correct password verifies; no upgrade needed for a fresh Argon2 hash.
        assert_eq!(verify_password("correct horse battery staple", &phc, ""), (true, false));
        // Wrong password fails.
        assert_eq!(verify_password("wrong", &phc, ""), (false, false));
    }

    #[test]
    fn legacy_sha256_verifies_and_flags_upgrade() {
        let salt = "some-legacy-salt";
        let legacy = legacy_sha256("hunter2", salt);
        // Legacy match → ok AND needs_upgrade.
        assert_eq!(verify_password("hunter2", &legacy, salt), (true, true));
        // Legacy mismatch → not ok, no upgrade.
        assert_eq!(verify_password("nope", &legacy, salt), (false, false));
    }

    #[test]
    fn each_hash_uses_a_fresh_salt() {
        let a = hash_password_argon2("samepw").unwrap();
        let b = hash_password_argon2("samepw").unwrap();
        assert_ne!(a, b, "Argon2 must embed a random per-password salt");
    }
}
