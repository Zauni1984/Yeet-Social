//! Axum middleware — JWT authentication extractor, client-IP helper and the
//! per-IP abuse guard for the auth and admin route classes.
use axum::{extract::{FromRequestParts, State, Request}, http::{request::Parts, header::AUTHORIZATION, HeaderMap}, middleware::Next, response::Response, RequestPartsExt};
use crate::services::rate_limit::{self, RateLimitOutcome};
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

        // Check token blacklist (logout / revoked session). Fail closed: if
        // Redis is unreachable we cannot prove the token was not revoked.
        match state.cache.is_blacklisted(&claims.jti).await {
            Ok(false) => {}
            Ok(true) => return Err(AppError::Unauthorised("Token has been revoked".into())),
            Err(e) => {
                tracing::error!("auth: blacklist lookup failed: {e}");
                return Err(AppError::Cache("auth temporarily unavailable".into()));
            }
        }

        Ok(AuthUser { address: claims.sub, jti: claims.jti })
    }
}

/// Like `AuthUser` but never rejects — unauthenticated requests yield `None`.
#[derive(Debug, Clone)]
pub struct OptionalAuth(pub Option<AuthUser>);

#[async_trait]
impl FromRequestParts<AppState> for OptionalAuth {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        Ok(OptionalAuth(AuthUser::from_request_parts(parts, state).await.ok()))
    }
}

/// Best-effort client address for rate limiting. nginx sets `X-Real-IP` and
/// appends to `X-Forwarded-For`; the first forwarded hop is the client.
/// Without a proxy header (local dev) every caller shares the "direct" bucket.
pub fn client_ip(headers: &HeaderMap) -> String {
    if let Some(v) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(first) = v.split(',').next().map(str::trim).filter(|s| !s.is_empty()) {
            return first.chars().take(64).collect();
        }
    }
    if let Some(v) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        let v = v.trim();
        if !v.is_empty() { return v.chars().take(64).collect(); }
    }
    "direct".into()
}

/// Two-window limit that maps to 429. Shared by the handler-level limits
/// (login per e-mail, register per IP, …).
pub async fn limit(state: &AppState, scope: &str, principal: &str,
                   burst_secs: u64, burst_cap: i64, sustained_secs: u64, sustained_cap: i64) -> AppResult<()> {
    match rate_limit::check_two_window(&state.cache, scope, principal, burst_secs, burst_cap, sustained_secs, sustained_cap).await {
        RateLimitOutcome::Allowed => Ok(()),
        _ => Err(AppError::RateLimited),
    }
}

/// Router-wide guard: every request to the authentication and admin route
/// classes is throttled per client IP before it reaches a handler, so a
/// brute force against passwords, nonces or the admin secret is capped no
/// matter which endpoint it targets. Generous for humans, hostile to scripts.
pub async fn abuse_guard(State(state): State<AppState>, mut req: Request, next: Next) -> Result<Response, AppError> {
    // Admin GETs may carry the secret in an `X-Admin-Secret` header instead
    // of `?secret=` so it stays out of access logs and browser history
    // (audit A-H4). The handlers keep reading `secret` from the query, so the
    // header is folded into the query string here.
    if req.method() == axum::http::Method::GET && req.uri().path().starts_with("/api/v1/admin/") {
        let hdr = req.headers().get("x-admin-secret").and_then(|v| v.to_str().ok()).map(|s| s.trim().to_string());
        if let Some(secret) = hdr.filter(|s| !s.is_empty()) {
            let has_query_secret = req.uri().query().map(|q| q.split('&').any(|kv| kv.starts_with("secret="))).unwrap_or(false);
            if !has_query_secret {
                let enc: String = secret.bytes().map(|b| match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
                    _ => format!("%{:02X}", b),
                }).collect();
                let pq = match req.uri().query() {
                    Some(q) if !q.is_empty() => format!("{}?{}&secret={}", req.uri().path(), q, enc),
                    _ => format!("{}?secret={}", req.uri().path(), enc),
                };
                let mut parts = req.uri().clone().into_parts();
                if let Ok(new_pq) = pq.parse::<axum::http::uri::PathAndQuery>() {
                    parts.path_and_query = Some(new_pq);
                    if let Ok(uri) = axum::http::Uri::from_parts(parts) { *req.uri_mut() = uri; }
                }
            }
        }
    }
    let path = req.uri().path();
    let class = if path.starts_with("/api/v1/admin/") {
        Some(("admin_ip", 60u64, 60i64, 3600u64, 600i64))
    } else if path.starts_with("/api/v1/auth/") {
        Some(("auth_ip", 60u64, 60i64, 3600u64, 900i64))
    } else {
        None
    };
    if let Some((scope, bw, bc, sw, sc)) = class {
        let ip = client_ip(req.headers());
        limit(&state, scope, &ip, bw, bc, sw, sc).await?;
    }
    Ok(next.run(req).await)
}
