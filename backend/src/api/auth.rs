//! Auth handlers — wallet-based login (works on web + Android + iOS).
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use crate::{AppError, AppResult, AppState, services::auth, models::ApiResponse};
use crate::api::middleware::AuthUser;

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct NonceRequest { pub address: String }

#[derive(Debug, Serialize)]
pub struct NonceResponse { pub nonce: String, pub message: String }

#[derive(Debug, Deserialize)]
pub struct VerifyRequest { pub address: String, pub signature: String, pub nonce: String }

#[derive(Debug, Serialize)]
pub struct TokenResponse { pub access_token: String, pub refresh_token: String, pub token_type: String }

#[derive(Debug, Deserialize)]
pub struct RefreshRequest { pub refresh_token: String }

// ── Handlers ──────────────────────────────────────────────────────────────────

/// Step 1: Request a nonce for a wallet address.
/// The client must sign this nonce with their private key.
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

    // Store nonce in Redis — expires in 10 minutes
    state.cache.set_nonce(&address, &nonce, Duration::from_secs(600)).await
        .map_err(|e| AppError::Cache(e.to_string()))?;

    Ok(Json(ApiResponse::ok(NonceResponse { nonce, message })))
}

/// Step 2: Verify EIP-191 signature and issue JWT tokens.
pub async fn verify_signature(
    State(state): State<AppState>,
    Json(req): Json<VerifyRequest>,
) -> AppResult<Json<ApiResponse<TokenResponse>>> {
    let address = req.address.to_lowercase();

    // Consume the nonce (single-use, atomic)
    let stored_nonce = state.cache.consume_nonce(&address).await
        .map_err(|e| AppError::Cache(e.to_string()))?
        .ok_or_else(|| AppError::Unauthorised("Nonce not found or expired".into()))?;

    if stored_nonce != req.nonce {
        return Err(AppError::Unauthorised("Nonce mismatch".into()));
    }

    // Verify EIP-191 signature — recover signer address
    let message = auth::sign_message(&req.nonce);
    let recovered = auth::recover_signer(&message, &req.signature)
        .map_err(|e| AppError::Unauthorised(format!("Signature verification failed: {e}")))?;

    if recovered != address {
        return Err(AppError::Unauthorised("Signature does not match address".into()));
    }

    // Upsert user in DB
    sqlx::query!(
        r#"INSERT INTO users (wallet_address) VALUES ($1)
           ON CONFLICT (wallet_address) DO UPDATE SET updated_at = NOW()"#,
        address
    )
    .execute(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    // Issue JWT pair
    let (access_token, refresh_token) = auth::issue_token_pair(&address, &state.jwt)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(ApiResponse::ok(TokenResponse {
        access_token,
        refresh_token,
        token_type: "Bearer".into(),
    })))
}

/// Refresh an access token using a valid refresh token.
pub async fn refresh_token(
    State(state): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> AppResult<Json<ApiResponse<TokenResponse>>> {
    let claims = auth::verify_refresh_token(&req.refresh_token, &state.jwt)
        .map_err(|e| AppError::Unauthorised(e.to_string()))?;

    if state.cache.is_blacklisted(&claims.jti).await.unwrap_or(false) {
        return Err(AppError::Unauthorised("Refresh token has been revoked".into()));
    }

    let (access_token, refresh_token) = auth::issue_token_pair(&claims.sub, &state.jwt)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(ApiResponse::ok(TokenResponse { access_token, refresh_token, token_type: "Bearer".into() })))
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn is_valid_address(address: &str) -> bool {
    address.starts_with("0x") && address.len() == 42 && address[2..].chars().all(|c| c.is_ascii_hexdigit())
}
