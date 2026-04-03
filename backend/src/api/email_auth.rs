//! Email-based authentication handlers.
//! Built: 2026-03-31 21:39 UTC
//! Users can register and log in with email + password as an alternative to wallet.
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, models::ApiResponse, services::auth};

#[derive(Debug, Deserialize)]
pub struct EmailRegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EmailLoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub username: String,
}

fn hash_password(password: &str, salt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{}{}", password, salt));
    format!("{:x}", hasher.finalize())
}

pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<EmailRegisterRequest>,
) -> AppResult<Json<ApiResponse<TokenResponse>>> {
    // Validate
    if req.email.is_empty() || !req.email.contains('@') {
        return Err(AppError::Validation("Invalid email address".into()));
    }
    if req.password.len() < 8 {
        return Err(AppError::Validation("Password must be at least 8 characters".into()));
    }

    // Check if email already exists
    let exists: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM users WHERE email = $1"
    )
    .bind(req.email.to_lowercase())
    .fetch_optional(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    if exists.is_some() {
        return Err(AppError::Validation("Email already registered".into()));
    }

    // Hash password with unique salt
    let salt = Uuid::new_v4().to_string();
    let hash = hash_password(&req.password, &salt);

    // Create username from email prefix
    let username = req.email
        .split('@')
        .next()
        .unwrap_or("user")
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .take(20)
        .collect::<String>();

    // Insert user (no wallet_address for email users)
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, password_salt, username, display_name)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (email) DO NOTHING
         RETURNING id"
    )
    .bind(req.email.to_lowercase())
    .bind(&hash)
    .bind(&salt)
    .bind(&username)
    .bind(req.display_name.unwrap_or_else(|| username.clone()))
    .fetch_one(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    let subject = format!("email:{}", user_id);
    let (access_token, refresh_token) = auth::issue_token_pair(&subject, &state.jwt)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(ApiResponse::ok(TokenResponse {
        access_token,
        refresh_token,
        token_type: "Bearer".into(),
        username: username.clone(),
    })))
}

pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<EmailLoginRequest>,
) -> AppResult<Json<ApiResponse<TokenResponse>>> {
    if req.email.is_empty() || req.password.is_empty() {
        return Err(AppError::Validation("Email and password required".into()));
    }

    // Fetch user
    let row = sqlx::query_as::<_, (Uuid, String, String, String)>(
        "SELECT id, password_hash, password_salt, COALESCE(username, 'user') FROM users WHERE email = $1"
    )
    .bind(req.email.to_lowercase())
    .fetch_optional(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    let (user_id, stored_hash, salt, username) = row
        .ok_or_else(|| AppError::Unauthorised("Invalid email or password".into()))?;

    let hash = hash_password(&req.password, &salt);
    if hash != stored_hash {
        return Err(AppError::Unauthorised("Invalid email or password".into()));
    }

    let subject = format!("email:{}", user_id);
    let (access_token, refresh_token) = auth::issue_token_pair(&subject, &state.jwt)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(ApiResponse::ok(TokenResponse {
        access_token,
        refresh_token,
        token_type: "Bearer".into(),
        username,
    })))
}