//! Centralised error handling for the Yeet API.
use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};
use serde_json::json;
use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Unauthorised: {0}")]
    Unauthorised(String),
    #[error("Forbidden: {0}")]
    Forbidden(String),
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Conflict: {0}")]
    Conflict(String),
    #[error("Rate limit exceeded")]
    RateLimited,
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Cache error: {0}")]
    Cache(String),
    #[error("Blockchain error: {0}")]
    Blockchain(String),
    #[error("Internal server error")]
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            AppError::Unauthorised(m)  => (StatusCode::UNAUTHORIZED,           "UNAUTHORISED",   m.clone()),
            AppError::Forbidden(m)     => (StatusCode::FORBIDDEN,              "FORBIDDEN",      m.clone()),
            AppError::Validation(m)    => (StatusCode::UNPROCESSABLE_ENTITY,   "VALIDATION",     m.clone()),
            AppError::NotFound(m)      => (StatusCode::NOT_FOUND,              "NOT_FOUND",      m.clone()),
            AppError::Conflict(m)      => (StatusCode::CONFLICT,               "CONFLICT",       m.clone()),
            AppError::RateLimited      => (StatusCode::TOO_MANY_REQUESTS,      "RATE_LIMITED",   "Too many requests.".into()),
            AppError::Database(e)      => { tracing::error!(error=%e, "DB error"); (StatusCode::INTERNAL_SERVER_ERROR, "DB_ERROR", "Database error".into()) }
            AppError::Cache(m)         => { tracing::error!(error=%m, "Cache error"); (StatusCode::INTERNAL_SERVER_ERROR, "CACHE_ERROR", "Cache error".into()) }
            AppError::Blockchain(m)    => (StatusCode::BAD_GATEWAY,            "BLOCKCHAIN",     m.clone()),
            AppError::Internal(m)      => { tracing::error!(error=%m, "Internal"); (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", "Internal error".into()) }
        };
        (status, Json(json!({ "success": false, "error": { "code": code, "message": message } }))).into_response()
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self { AppError::Internal(e.to_string()) }
}
