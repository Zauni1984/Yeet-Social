//! Email-based authentication + verification (DSGVO double-opt-in).
use axum::{extract::State, Json};
use chrono::{Duration as ChronoDuration, Utc};
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, api::middleware::AuthUser, models::ApiResponse, services::{auth, email as email_svc}};

#[derive(Debug, Deserialize)]
pub struct EmailRegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: Option<String>,
    pub consent: Option<bool>,
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

fn hash_password(password: &str, salt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{}{}", password, salt));
    format!("{:x}", hasher.finalize())
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

    let salt = Uuid::new_v4().to_string();
    let hash = hash_password(&req.password, &salt);
    let username_base = email_lower.split('@').next().unwrap_or("user")
        .chars().filter(|c| c.is_alphanumeric() || *c == '_').take(20).collect::<String>();
    let username = unique_username(&state, &username_base).await?;

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

    if hash_password(&req.password, &salt) != stored_hash {
        return Err(AppError::Unauthorised("Invalid email or password".into()));
    }

    let subject = format!("email:{}", user_id);
    let (access_token, refresh_token) = auth::issue_token_pair(&subject, &state.jwt)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(ApiResponse::ok(TokenResponse {
        access_token, refresh_token,
        token_type: "Bearer".into(),
        username,
        email_verified: verified_at.is_some(),
    })))
}

pub async fn verify_email(
    State(state): State<AppState>,
    Json(req): Json<VerifyEmailRequest>,
) -> AppResult<Json<ApiResponse<SimpleOk>>> {
    if req.token.len() < 16 {
        return Err(AppError::Validation("Invalid token".into()));
    }

    let row: Option<(Uuid, String, chrono::DateTime<Utc>)> = sqlx::query_as(
        "SELECT user_id, email, expires_at FROM email_verification_tokens WHERE token = $1"
    )
    .bind(&req.token)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;

    let (user_id, email, expires_at) = row
        .ok_or_else(|| AppError::NotFound("Verification link invalid or already used".into()))?;

    if expires_at < Utc::now() {
        sqlx::query("DELETE FROM email_verification_tokens WHERE token = $1")
            .bind(&req.token).execute(state.db.pool()).await.map_err(AppError::Database)?;
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

    Ok(Json(ApiResponse::ok(SimpleOk { ok: true })))
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
