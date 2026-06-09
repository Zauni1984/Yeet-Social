//! Push subscription management + public VAPID key.
//!
//! Endpoints:
//!   GET    /api/v1/push/config       → public VAPID key + whether
//!                                      pushes are enabled at all
//!   POST   /api/v1/me/push/subscribe → register a new subscription
//!   DELETE /api/v1/me/push/subscribe → remove (by endpoint)
//!
//! Routed by main.rs. The actual push-fan-out lives in
//! `services::push` and is invoked by the messaging handlers when a
//! recipient is offline (no WS connection).

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;
use crate::api::conversations::caller_user_id;
use crate::services::push;

#[derive(Debug, Serialize)]
pub struct PushPublicConfig {
    pub enabled: bool,
    pub vapid_public_key: Option<String>,
}

/// Service-worker JS is baked into the binary so the operator
/// doesn't need to keep a separate static file in sync with the
/// nginx docroot. Served at `/sw.js` with the strictest scope of `/`
/// and no caching so users always run the latest copy after a
/// deploy.
pub async fn service_worker_js() -> axum::response::Response {
    use axum::http::{header, HeaderValue, StatusCode};
    let body = include_str!("../static_assets/sw.js");
    let mut resp = axum::response::Response::new(body.into());
    *resp.status_mut() = StatusCode::OK;
    let h = resp.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/javascript; charset=utf-8"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store, max-age=0"));
    // Allow the SW to register with the broadest possible scope.
    h.insert("Service-Worker-Allowed", HeaderValue::from_static("/"));
    resp
}

pub async fn config_status(
    State(_state): State<AppState>,
) -> Json<ApiResponse<PushPublicConfig>> {
    match push::config_from_env() {
        Some(cfg) => Json(ApiResponse::ok(PushPublicConfig {
            enabled: true,
            vapid_public_key: Some(cfg.public_key_b64),
        })),
        None => Json(ApiResponse::ok(PushPublicConfig {
            enabled: false,
            vapid_public_key: None,
        })),
    }
}

#[derive(Debug, Deserialize)]
pub struct SubscribeRequest {
    pub endpoint: String,
    /// P-256 ECDH public key from the browser's PushSubscription
    /// (base64url, 65 bytes uncompressed).
    pub p256dh_key: String,
    /// Browser-generated 16-byte auth secret (base64url).
    pub auth_key: String,
    pub user_agent: Option<String>,
}

pub async fn subscribe(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<SubscribeRequest>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    if req.endpoint.len() > 2048
        || req.p256dh_key.len() > 200
        || req.auth_key.len() > 200
    {
        return Err(AppError::Validation("subscription payload too large".into()));
    }
    if !req.endpoint.starts_with("https://") {
        return Err(AppError::Validation("endpoint must be https".into()));
    }
    let me = caller_user_id(&state, &auth).await?;
    let ua = req.user_agent.as_deref().map(|s| s.chars().take(200).collect::<String>());

    sqlx::query(
        "INSERT INTO push_subscriptions
            (user_id, endpoint, p256dh_key, auth_key, user_agent)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (endpoint) DO UPDATE SET
             user_id      = EXCLUDED.user_id,
             p256dh_key   = EXCLUDED.p256dh_key,
             auth_key     = EXCLUDED.auth_key,
             user_agent   = EXCLUDED.user_agent,
             expired_at   = NULL,
             last_seen_at = NOW()"
    )
    .bind(me)
    .bind(&req.endpoint)
    .bind(&req.p256dh_key)
    .bind(&req.auth_key)
    .bind(ua)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok("subscribed")))
}

#[derive(Debug, Deserialize)]
pub struct UnsubscribeRequest {
    pub endpoint: String,
}

pub async fn unsubscribe(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<UnsubscribeRequest>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    let me = caller_user_id(&state, &auth).await?;
    sqlx::query("DELETE FROM push_subscriptions WHERE user_id = $1 AND endpoint = $2")
        .bind(me).bind(&req.endpoint)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok("unsubscribed")))
}

/// Internal helper called by the messaging layer when a recipient is
/// offline (no WS). Fans tickle pushes to every active subscription
/// the user has registered. Best-effort: any push that fails with
/// 404/410 marks the row expired so future sends skip it; transient
/// errors are logged and ignored. Never blocks the caller.
pub async fn push_tickle_to_users(state: &AppState, user_ids: &[Uuid]) {
    let cfg = match push::config_from_env() {
        Some(c) => c,
        None => return, // pushes not configured — no-op
    };
    if user_ids.is_empty() { return; }

    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, endpoint FROM push_subscriptions
          WHERE user_id = ANY($1) AND expired_at IS NULL"
    )
    .bind(user_ids)
    .fetch_all(state.db.pool()).await.unwrap_or_default();
    if rows.is_empty() { return; }

    // Build the HTTP client once. Native-rustls so we don't pull
    // OpenSSL on the deploy host.
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "push: http client build failed");
            return;
        }
    };

    for (sub_id, endpoint) in rows {
        let cfg = cfg.clone();
        let client = client.clone();
        let pool = state.db.pool().clone();
        // Each push fires off in its own task so the calling handler
        // returns immediately. We deliberately don't await on them.
        tokio::spawn(async move {
            match push::send_tickle(&cfg, &endpoint, &client).await {
                Ok(true) => {
                    let _ = sqlx::query(
                        "UPDATE push_subscriptions SET last_seen_at = NOW() WHERE id = $1"
                    ).bind(sub_id).execute(&pool).await;
                }
                Ok(false) => {
                    // 404/410 → mark dead so we stop trying.
                    let _ = sqlx::query(
                        "UPDATE push_subscriptions SET expired_at = NOW() WHERE id = $1"
                    ).bind(sub_id).execute(&pool).await;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "push tickle failed");
                }
            }
        });
    }
}
