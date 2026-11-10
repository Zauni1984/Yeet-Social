//! Developer API keys — the public interface for autoposters, AI agents and
//! other integrations (docs/api/README.md, frontend/api.html).
//!
//! Flow: a signed-in user with a verified e-mail applies for a key (name,
//! purpose, kind, scopes). The secret `yeet_sk_…` is shown ONCE; only its
//! SHA-256 is stored. The key is `pending` until an admin approves it in
//! admin.html; `rejected`/`revoked` keys are refused. An approved key
//! authenticates like a Bearer token (api/middleware.rs) and acts as the
//! owner, but only on the endpoints its scopes allow (`route_allowed`) and
//! within its per-minute / per-day limits. Posts created through a key are
//! tagged with the key's kind so the UI can label automated / AI content.
use axum::{extract::{Path, Query, State}, Json};
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;
use crate::api::admin_mod::check_admin_secret;

pub const PREFIX: &str = "yeet_sk_";
pub const SCOPES: &[&str] = &["read", "write:posts", "write:interactions"];
pub const KINDS: &[&str] = &["autoposter", "ai", "other"];
const MAX_ACTIVE_KEYS: i64 = 5;

/// What the auth middleware knows about the key behind a request.
#[derive(Debug, Clone)]
pub struct ApiKeyCtx {
    pub id: Uuid,
    pub kind: String,
    pub scopes: Vec<String>,
}

pub fn hash_secret(secret: &str) -> String {
    hex::encode(Sha256::digest(secret.as_bytes()))
}

fn new_secret() -> String {
    let mut b = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut b);
    format!("{PREFIX}{}", hex::encode(b))
}

/// Which endpoints a key with these scopes may call. Everything not listed
/// here is refused (points, tips, paper wallets, messaging, settings,
/// sessions, key management, admin …) — a leaked key can post and read,
/// never move value or change the account.
pub fn route_allowed(method: &str, path: &str, scopes: &[String]) -> bool {
    let has = |s: &str| scopes.iter().any(|x| x == s);
    let rest = path.strip_prefix("/api/v1/").unwrap_or("");
    let segs: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    match (method, segs.as_slice()) {
        // whoami for the key itself — always allowed
        ("GET", ["me", "api-keys", "self"]) => true,
        // ── read ──
        ("GET", ["feed"]) | ("GET", ["feed", "following"]) | ("GET", ["feed", "permanent"]) => has("read"),
        ("GET", ["posts", _]) | ("GET", ["posts", _, "comments"]) => has("read"),
        ("GET", ["users", _]) | ("GET", ["users", _, "posts"]) | ("GET", ["users", _, "followers"]) | ("GET", ["users", _, "following"]) => has("read"),
        ("GET", ["search"]) | ("GET", ["notifications"]) | ("GET", ["notifications", "unread-count"]) => has("read"),
        ("GET", ["me", "permanent"]) | ("GET", ["profile", _, "permanent"]) => has("read"),
        ("GET", ["boards"]) | ("GET", ["boards", _]) | ("GET", ["lives", "active"]) | ("GET", ["lives", "scheduled"]) => has("read"),
        ("GET", ["points", "rate"]) | ("GET", ["tokens", "pool"]) | ("GET", ["config"]) => has("read"),
        // ── write:posts ──
        ("POST", ["posts"]) | ("DELETE", ["posts", _]) | ("PATCH", ["posts", _, "visibility"]) => has("write:posts"),
        ("POST", ["uploads", "post-media"]) => has("write:posts"),
        ("POST", ["scheduled-posts"]) | ("GET", ["scheduled-posts", "mine"]) | ("DELETE", ["scheduled-posts", _]) => has("write:posts"),
        // ── write:interactions ──
        ("POST", ["posts", _, "like"]) | ("POST", ["posts", _, "unlike"]) | ("POST", ["posts", _, "reshare"]) | ("POST", ["posts", _, "comments"]) => has("write:interactions"),
        ("POST", ["users", _, "follow"]) | ("POST", ["users", _, "unfollow"]) => has("write:interactions"),
        ("POST", ["notifications", "read"]) | ("POST", ["notifications", _, "read"]) => has("write:interactions"),
        _ => false,
    }
}

/// Resolve a presented `yeet_sk_…` secret to the owner's auth subject and
/// the key context. Approved keys only; the route must be within the key's
/// scopes (checked BEFORE the rate limit, so refused calls cost no quota);
/// per-key rate limits apply.
pub async fn authenticate(state: &AppState, secret: &str, method: &str, path: &str) -> AppResult<(String, ApiKeyCtx)> {
    let h = hash_secret(secret);
    let row: Option<(Uuid, Uuid, String, Vec<String>, String, i32, i32)> = sqlx::query_as(
        "SELECT id, user_id, kind, scopes, status, rate_per_minute, rate_per_day
           FROM api_keys WHERE key_hash = $1"
    )
    .bind(&h)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let Some((id, user_id, kind, scopes, status, per_min, per_day)) = row else {
        return Err(AppError::Unauthorised("Invalid API key".into()));
    };
    if status != "approved" {
        return Err(AppError::Unauthorised(format!("API key is {status}")));
    }
    if !route_allowed(method, path, &scopes) {
        return Err(AppError::Forbidden("API key scope does not allow this endpoint".into()));
    }
    crate::api::middleware::limit(state, "api_key", &id.to_string(), 60, per_min as i64, 86400, per_day as i64).await?;
    let _ = sqlx::query("UPDATE api_keys SET last_used_at = NOW(), use_count = use_count + 1 WHERE id = $1")
        .bind(id).execute(state.db.pool()).await;
    // The `email:<uuid>` subject form resolves for every account type.
    Ok((format!("email:{user_id}"), ApiKeyCtx { id, kind, scopes }))
}

// ── user-facing key management (JWT only; keys cannot manage keys) ──

#[derive(Debug, Deserialize)]
pub struct CreateKeyRequest {
    pub name: String,
    pub purpose: String,
    pub kind: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CreatedKey {
    pub id: Uuid,
    /// Shown once. Never stored in clear.
    pub secret: String,
    pub key_prefix: String,
    pub status: String,
    pub kind: String,
    pub scopes: Vec<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct KeyRow {
    pub id: Uuid,
    pub name: String,
    pub purpose: String,
    pub kind: String,
    pub scopes: Vec<String>,
    pub key_prefix: String,
    pub status: String,
    pub rate_per_minute: i32,
    pub rate_per_day: i32,
    pub created_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decision_note: Option<String>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub use_count: i64,
}

fn no_key_auth(auth: &AuthUser) -> AppResult<()> {
    if auth.api_key.is_some() {
        return Err(AppError::Forbidden("API keys cannot manage API keys".into()));
    }
    Ok(())
}

/// POST /api/v1/me/api-keys — apply for a key (pending until approved).
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateKeyRequest>,
) -> AppResult<Json<ApiResponse<CreatedKey>>> {
    no_key_auth(&auth)?;
    let user_id = crate::api::users::resolve_user_id(&state, &auth.address).await?;
    let name = req.name.trim();
    let purpose = req.purpose.trim();
    if name.chars().count() < 2 || name.chars().count() > 64 {
        return Err(AppError::Validation("name must be 2–64 characters".into()));
    }
    if purpose.chars().count() < 20 || purpose.chars().count() > 1000 {
        return Err(AppError::Validation("purpose must be 20–1000 characters: describe what the integration posts or reads".into()));
    }
    let kind = req.kind.trim().to_lowercase();
    if !KINDS.contains(&kind.as_str()) {
        return Err(AppError::Validation("kind must be autoposter, ai or other".into()));
    }
    let mut scopes: Vec<String> = req.scopes.iter().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect();
    if scopes.is_empty() { scopes.push("read".into()); }
    scopes.sort(); scopes.dedup();
    if let Some(bad) = scopes.iter().find(|s| !SCOPES.contains(&s.as_str())) {
        return Err(AppError::Validation(format!("unknown scope '{bad}' (allowed: read, write:posts, write:interactions)")));
    }
    let verified: bool = sqlx::query_scalar("SELECT email_verified_at IS NOT NULL FROM users WHERE id = $1")
        .bind(user_id).fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
    if !verified {
        return Err(AppError::Forbidden("Verify your e-mail address before applying for API access".into()));
    }
    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_keys WHERE user_id = $1 AND status <> 'revoked'"
    ).bind(user_id).fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
    if active >= MAX_ACTIVE_KEYS {
        return Err(AppError::Validation(format!("at most {MAX_ACTIVE_KEYS} keys per account; revoke one first")));
    }
    let secret = new_secret();
    let prefix: String = secret.chars().take(PREFIX.len() + 6).collect();
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO api_keys (user_id, name, purpose, kind, scopes, key_prefix, key_hash)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id"
    )
    .bind(user_id).bind(name).bind(purpose).bind(&kind).bind(&scopes).bind(&prefix).bind(hash_secret(&secret))
    .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(CreatedKey {
        id, secret, key_prefix: prefix, status: "pending".into(), kind, scopes,
    })))
}

/// GET /api/v1/me/api-keys
pub async fn list_mine(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<Vec<KeyRow>>>> {
    no_key_auth(&auth)?;
    let user_id = crate::api::users::resolve_user_id(&state, &auth.address).await?;
    let rows: Vec<KeyRow> = sqlx::query_as(
        "SELECT id, name, purpose, kind, scopes, key_prefix, status, rate_per_minute, rate_per_day,
                created_at, decided_at, decision_note, last_used_at, use_count
           FROM api_keys WHERE user_id = $1 ORDER BY created_at DESC"
    )
    .bind(user_id)
    .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(rows)))
}

/// DELETE /api/v1/me/api-keys/:id — revoke (final).
pub async fn revoke_mine(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    no_key_auth(&auth)?;
    let user_id = crate::api::users::resolve_user_id(&state, &auth.address).await?;
    let n = sqlx::query(
        "UPDATE api_keys SET status = 'revoked', decided_at = NOW(), decided_by = 'owner'
          WHERE id = $1 AND user_id = $2 AND status <> 'revoked'"
    )
    .bind(id).bind(user_id)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;
    if n.rows_affected() == 0 {
        return Err(AppError::NotFound("Key not found".into()));
    }
    Ok(Json(ApiResponse::ok("revoked")))
}

#[derive(Debug, Serialize)]
pub struct WhoAmI {
    pub key_id: Uuid,
    pub kind: String,
    pub scopes: Vec<String>,
    pub owner_user_id: Uuid,
    pub owner_username: Option<String>,
}

/// GET /api/v1/me/api-keys/self — what the presented key is allowed to do.
pub async fn whoami(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<WhoAmI>>> {
    let Some(k) = auth.api_key.clone() else {
        return Err(AppError::Validation("This endpoint is for API-key requests".into()));
    };
    let user_id = crate::api::users::resolve_user_id(&state, &auth.address).await?;
    let username: Option<String> = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(user_id).fetch_optional(state.db.pool()).await.map_err(AppError::Database)?.flatten();
    Ok(Json(ApiResponse::ok(WhoAmI { key_id: k.id, kind: k.kind, scopes: k.scopes, owner_user_id: user_id, owner_username: username })))
}

// ── admin ──

#[derive(Debug, Deserialize)]
pub struct AdminListQuery {
    pub secret: String,
    pub status: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AdminKeyRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub username: Option<String>,
    pub email: Option<String>,
    pub name: String,
    pub purpose: String,
    pub kind: String,
    pub scopes: Vec<String>,
    pub key_prefix: String,
    pub status: String,
    pub rate_per_minute: i32,
    pub rate_per_day: i32,
    pub created_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decision_note: Option<String>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub use_count: i64,
}

const ADMIN_SELECT: &str = "SELECT k.id, k.user_id, u.username, u.email, k.name, k.purpose, k.kind, k.scopes, k.key_prefix,
       k.status, k.rate_per_minute, k.rate_per_day, k.created_at, k.decided_at, k.decision_note, k.last_used_at, k.use_count
  FROM api_keys k JOIN users u ON u.id = k.user_id";

/// GET /api/v1/admin/api-keys?status=pending|approved|rejected|revoked|all
pub async fn admin_list(
    State(state): State<AppState>,
    Query(q): Query<AdminListQuery>,
) -> AppResult<Json<ApiResponse<Vec<AdminKeyRow>>>> {
    check_admin_secret(&q.secret)?;
    let status = q.status.unwrap_or_else(|| "pending".into());
    if !["pending", "approved", "rejected", "revoked", "all"].contains(&status.as_str()) {
        return Err(AppError::Validation("invalid status filter".into()));
    }
    let rows: Vec<AdminKeyRow> = if status == "all" {
        // Dynamic SQL audited for sqlx 0.9 (`SqlSafeStr`): only constant fragments or
        // whitelisted identifiers are interpolated; every user-supplied value is bound.
        sqlx::query_as(sqlx::AssertSqlSafe(format!("{ADMIN_SELECT} ORDER BY k.created_at DESC LIMIT 500")))
            .fetch_all(state.db.pool()).await.map_err(AppError::Database)?
    } else {
        sqlx::query_as(sqlx::AssertSqlSafe(format!("{ADMIN_SELECT} WHERE k.status = $1 ORDER BY k.created_at DESC LIMIT 500")))
            .bind(&status)
            .fetch_all(state.db.pool()).await.map_err(AppError::Database)?
    };
    Ok(Json(ApiResponse::ok(rows)))
}

#[derive(Debug, Deserialize)]
pub struct AdminDecision {
    pub secret: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub rate_per_minute: Option<i32>,
    #[serde(default)]
    pub rate_per_day: Option<i32>,
}

async fn admin_set_status(
    state: &AppState,
    id: Uuid,
    body: &AdminDecision,
    new_status: &str,
    from: &[&str],
    action: &str,
) -> AppResult<()> {
    check_admin_secret(&body.secret)?;
    if let Some(m) = body.rate_per_minute { if !(1..=600).contains(&m) { return Err(AppError::Validation("rate_per_minute must be 1–600".into())); } }
    if let Some(d) = body.rate_per_day { if !(1..=200_000).contains(&d) { return Err(AppError::Validation("rate_per_day must be 1–200000".into())); } }
    let note: Option<String> = body.note.as_deref().map(|n| n.chars().take(500).collect()).filter(|n: &String| !n.is_empty());
    let from: Vec<String> = from.iter().map(|s| s.to_string()).collect();
    let row: Option<(Uuid, Option<String>)> = sqlx::query_as(
        "UPDATE api_keys k
            SET status = $2, decided_at = NOW(), decided_by = 'admin', decision_note = $3,
                rate_per_minute = COALESCE($4, rate_per_minute),
                rate_per_day    = COALESCE($5, rate_per_day)
          WHERE k.id = $1 AND k.status = ANY($6)
         RETURNING k.user_id, (SELECT username FROM users WHERE id = k.user_id)"
    )
    .bind(id).bind(new_status).bind(&note).bind(body.rate_per_minute).bind(body.rate_per_day).bind(&from)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let Some((user_id, username)) = row else {
        return Err(AppError::NotFound("Key not found in a state that allows this action".into()));
    };
    crate::api::admin_mod::record_action(
        state.db.pool(), Some(user_id), username.as_deref(), action, None, note.as_deref(), None, Some("admin"),
    ).await;
    Ok(())
}

/// POST /api/v1/admin/api-keys/:id/approve
pub async fn admin_approve(State(state): State<AppState>, Path(id): Path<Uuid>, Json(b): Json<AdminDecision>) -> AppResult<Json<ApiResponse<&'static str>>> {
    admin_set_status(&state, id, &b, "approved", &["pending", "rejected", "approved"], "api_key_approve").await?;
    Ok(Json(ApiResponse::ok("approved")))
}
/// POST /api/v1/admin/api-keys/:id/reject
pub async fn admin_reject(State(state): State<AppState>, Path(id): Path<Uuid>, Json(b): Json<AdminDecision>) -> AppResult<Json<ApiResponse<&'static str>>> {
    admin_set_status(&state, id, &b, "rejected", &["pending"], "api_key_reject").await?;
    Ok(Json(ApiResponse::ok("rejected")))
}
/// POST /api/v1/admin/api-keys/:id/revoke
pub async fn admin_revoke(State(state): State<AppState>, Path(id): Path<Uuid>, Json(b): Json<AdminDecision>) -> AppResult<Json<ApiResponse<&'static str>>> {
    admin_set_status(&state, id, &b, "revoked", &["pending", "approved", "rejected"], "api_key_revoke").await?;
    Ok(Json(ApiResponse::ok("revoked")))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn s(v: &[&str]) -> Vec<String> { v.iter().map(|x| x.to_string()).collect() }

    #[test]
    fn scopes_gate_routes() {
        let read = s(&["read"]);
        let write = s(&["read", "write:posts"]);
        let inter = s(&["write:interactions"]);
        assert!(route_allowed("GET", "/api/v1/feed", &read));
        assert!(route_allowed("GET", "/api/v1/posts/abc/comments", &read));
        assert!(!route_allowed("POST", "/api/v1/posts", &read));
        assert!(route_allowed("POST", "/api/v1/posts", &write));
        assert!(route_allowed("POST", "/api/v1/uploads/post-media", &write));
        assert!(!route_allowed("POST", "/api/v1/posts/abc/like", &write));
        assert!(route_allowed("POST", "/api/v1/posts/abc/like", &inter));
        // value-moving and account endpoints are never reachable with a key
        for (m, p) in [("POST", "/api/v1/tips"), ("POST", "/api/v1/points/convert"), ("POST", "/api/v1/paper-wallets"),
                       ("POST", "/api/v1/posts/abc/unlock"), ("GET", "/api/v1/me/sessions"), ("PATCH", "/api/v1/users/me"),
                       ("POST", "/api/v1/me/api-keys"), ("GET", "/api/v1/conversations"), ("GET", "/api/v1/admin/stats")] {
            assert!(!route_allowed(m, p, &s(&["read", "write:posts", "write:interactions"])), "{m} {p} must be refused");
        }
        assert!(route_allowed("GET", "/api/v1/me/api-keys/self", &s(&[])));
    }

    #[test]
    fn secret_shape_and_hash() {
        let sec = new_secret();
        assert!(sec.starts_with(PREFIX) && sec.len() == PREFIX.len() + 48);
        assert_eq!(hash_secret(&sec).len(), 64);
        assert_ne!(hash_secret(&sec), hash_secret(&new_secret()));
    }
}
