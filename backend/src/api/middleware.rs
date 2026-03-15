//! Axum middleware — JWT authentication extractor.
use axum::{extract::{FromRequestParts, State}, http::{request::Parts, header::AUTHORIZATION}, RequestPartsExt};
use async_trait::async_trait;
use crate::{AppError, AppResult, AppState, services::auth::verify_access_token};

/// Authenticated user extracted from JWT Bearer token.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub address: String,
    pub jti: String,
}

#[async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> AppResult<Self> {
        let auth_header = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::Unauthorised("Missing Authorization header".into()))?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or_else(|| AppError::Unauthorised("Invalid Authorization format — expected 'Bearer <token>'".into()))?;

        let claims = verify_access_token(token, &state.jwt)
            .map_err(|e| AppError::Unauthorised(e.to_string()))?;

        // Check token blacklist (logout)
        if state.cache.is_blacklisted(&claims.jti).await.unwrap_or(false) {
            return Err(AppError::Unauthorised("Token has been revoked".into()));
        }

        Ok(AuthUser { address: claims.sub, jti: claims.jti })
    }
}
