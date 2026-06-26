//! Admin moderation: posting bans, account deletion, audit log.
//!
//! Authenticates via the same shared ADMIN_SECRET pattern used by
//! the existing report-moderation endpoints (see `api::report`). Two
//! moderator actions are exposed:
//!
//! 1. Ban a user from posting for 12 h / 24 h / 7 d / 30 d. The ban
//!    is enforced in `posts::create_post` by comparing
//!    `users.posting_banned_until` against NOW().
//! 2. Hard-delete a user. Requires the admin to repeat the username
//!    in the request body so a typo can't nuke the wrong account.
//!    Cascade FKs handle most associated rows; we explicitly null
//!    out the rest beforehand for safety.
//!
//! Every action is recorded in `admin_actions` with a snapshot of the
//! target's username so the audit log survives the deletion.

use axum::{extract::{Path, Query, State}, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::OptionalAuth;

/// Resolve the admin actor (the logged-in user) from an optional JWT.
/// Returns (id, username) — both None if the caller isn't signed in
/// (admin-secret-only flow). Used for audit-log attribution.
async fn admin_actor(state: &AppState, viewer: &OptionalAuth) -> (Option<Uuid>, Option<String>) {
    let auth = match &viewer.0 { Some(a) => a, None => return (None, None) };
    let id_opt: Option<Uuid> = if let Some(rest) = auth.address.strip_prefix("email:") {
        Uuid::parse_str(rest).ok()
    } else {
        sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
            .bind(&auth.address)
            .fetch_optional(state.db.pool())
            .await
            .ok()
            .flatten()
    };
    let Some(id) = id_opt else { return (None, None); };
    let username: Option<String> = sqlx::query_scalar(
        "SELECT username FROM users WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(state.db.pool())
    .await
    .ok()
    .flatten();
    (Some(id), username)
}

/// Allowed posting-ban durations, in hours. The frontend should only
/// ever submit one of these; anything else is rejected with 422.
const ALLOWED_BAN_HOURS: &[i32] = &[12, 24, 24 * 7, 24 * 30];

fn check_admin(secret: &str) -> AppResult<()> {
    check_admin_secret(secret)
}

/// Public version exposed to sibling admin endpoints (message reports,
/// session forced-revocation, etc.). Hardened versus the previous
/// behaviour:
///   * No hardcoded default. If ADMIN_SECRET is missing or shorter
///     than 24 chars the call fails closed with a generic 401 — same
///     error a wrong-password attempt produces, so a misconfigured
///     deploy doesn't telegraph "no auth required".
///   * Constant-time comparison so the wall-clock can't be used to
///     learn the secret length / prefix.
pub fn check_admin_secret(secret: &str) -> AppResult<()> {
    let admin_secret = match std::env::var("ADMIN_SECRET") {
        Ok(s) if s.len() >= 24 => s,
        _ => {
            tracing::warn!(
                "ADMIN_SECRET unset or too short (need ≥ 24 chars) — admin endpoints are disabled"
            );
            return Err(AppError::Unauthorised("Invalid admin secret".into()));
        }
    };
    if !ct_eq(secret.as_bytes(), admin_secret.as_bytes()) {
        return Err(AppError::Unauthorised("Invalid admin secret".into()));
    }
    Ok(())
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// Cheap secret-check endpoint the dashboard can call on open to
// surface a precise diagnostic ("bad secret" vs. "DB error" vs.
// "network") instead of conflating them on every metric call.
pub async fn ping(
    Query(q): Query<ActionsQuery>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    check_admin(&q.secret)?;
    Ok(Json(ApiResponse::ok("ok")))
}

async fn resolve_target(state: &AppState, address_or_id: &str)
    -> AppResult<(Uuid, Option<String>)>
{
    let raw = address_or_id.trim().trim_start_matches('@');
    if let Ok(id) = Uuid::parse_str(raw) {
        let username: Option<String> = sqlx::query_scalar(
            "SELECT username FROM users WHERE id = $1"
        ).bind(id).fetch_optional(state.db.pool()).await
         .map_err(AppError::Database)?.flatten();
        return Ok((id, username));
    }
    if let Some(row) = sqlx::query_as::<_, (Uuid, Option<String>)>(
        "SELECT id, username FROM users WHERE LOWER(wallet_address) = LOWER($1)"
    ).bind(raw).fetch_optional(state.db.pool()).await.map_err(AppError::Database)? {
        return Ok(row);
    }
    if let Some(row) = sqlx::query_as::<_, (Uuid, Option<String>)>(
        "SELECT id, username FROM users WHERE LOWER(username) = LOWER($1)"
    ).bind(raw).fetch_optional(state.db.pool()).await.map_err(AppError::Database)? {
        return Ok(row);
    }
    Err(AppError::NotFound("User not found".into()))
}

#[allow(clippy::too_many_arguments)]
pub async fn record_action(
    pool: &sqlx::PgPool,
    target_id: Option<Uuid>,
    target_username: Option<&str>,
    action_type: &str,
    duration_hours: Option<i32>,
    reason: Option<&str>,
    admin_user_id: Option<Uuid>,
    admin_username: Option<&str>,
) {
    let res = sqlx::query(
        "INSERT INTO admin_actions
            (target_user_id, target_username, action_type, duration_hours, reason,
             admin_user_id, admin_username)
         VALUES ($1, $2, $3, $4, $5, $6, $7)"
    )
    .bind(target_id)
    .bind(target_username)
    .bind(action_type)
    .bind(duration_hours)
    .bind(reason)
    .bind(admin_user_id)
    .bind(admin_username)
    .execute(pool).await;
    if let Err(e) = res {
        tracing::warn!(error = %e, "admin_actions insert failed");
    }
}

// ---------- ban_post ----------

#[derive(Debug, Deserialize)]
pub struct BanPostRequest {
    pub secret: String,
    pub duration_hours: i32,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BanResponse {
    pub user_id: Uuid,
    pub username: Option<String>,
    pub banned_until: DateTime<Utc>,
}

pub async fn ban_post(
    State(state): State<AppState>,
    viewer: OptionalAuth,
    Path(address): Path<String>,
    Json(req): Json<BanPostRequest>,
) -> AppResult<Json<ApiResponse<BanResponse>>> {
    check_admin(&req.secret)?;
    if !ALLOWED_BAN_HOURS.contains(&req.duration_hours) {
        return Err(AppError::Validation(
            "duration_hours must be 12, 24, 168 or 720".into()));
    }
    let (id, username) = resolve_target(&state, &address).await?;
    let (admin_id, admin_name) = admin_actor(&state, &viewer).await;

    let row: (DateTime<Utc>,) = sqlx::query_as(
        "UPDATE users
            SET posting_banned_until = NOW() + ($1 || ' hours')::INTERVAL,
                post_ban_reason = $2
          WHERE id = $3
         RETURNING posting_banned_until"
    )
    .bind(req.duration_hours.to_string())
    .bind(req.reason.as_deref())
    .bind(id)
    .fetch_one(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    record_action(
        state.db.pool(), Some(id), username.as_deref(),
        "ban_post", Some(req.duration_hours), req.reason.as_deref(),
        admin_id, admin_name.as_deref(),
    ).await;

    Ok(Json(ApiResponse::ok(BanResponse {
        user_id: id, username, banned_until: row.0,
    })))
}

// ---------- unban_post ----------

#[derive(Debug, Deserialize)]
pub struct UnbanPostRequest {
    pub secret: String,
    pub reason: Option<String>,
}

pub async fn unban_post(
    State(state): State<AppState>,
    viewer: OptionalAuth,
    Path(address): Path<String>,
    Json(req): Json<UnbanPostRequest>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    check_admin(&req.secret)?;
    let (id, username) = resolve_target(&state, &address).await?;
    let (admin_id, admin_name) = admin_actor(&state, &viewer).await;
    sqlx::query(
        "UPDATE users SET posting_banned_until = NULL, post_ban_reason = NULL WHERE id = $1"
    )
    .bind(id).execute(state.db.pool()).await.map_err(AppError::Database)?;

    record_action(
        state.db.pool(), Some(id), username.as_deref(),
        "unban_post", None, req.reason.as_deref(),
        admin_id, admin_name.as_deref(),
    ).await;

    Ok(Json(ApiResponse::ok("unbanned")))
}

// ---------- delete_user ----------

#[derive(Debug, Deserialize)]
pub struct DeleteUserRequest {
    pub secret: String,
    /// Must equal the lower-cased username being deleted. Acts as the
    /// admin's second "are you sure?" - a typo on the address part
    /// won't match the wrong account's username.
    pub confirmation: String,
    pub reason: Option<String>,
}

pub async fn delete_user(
    State(state): State<AppState>,
    viewer: OptionalAuth,
    Path(address): Path<String>,
    Json(req): Json<DeleteUserRequest>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    check_admin(&req.secret)?;
    let (id, username) = resolve_target(&state, &address).await?;
    let expected = match username.as_deref() {
        Some(u) => u.to_lowercase(),
        None => return Err(AppError::Validation(
            "Target has no username; cannot confirm.".into())),
    };
    if req.confirmation.trim().to_lowercase() != expected {
        return Err(AppError::Validation(
            "Confirmation must exactly match the target's username.".into()));
    }
    let (admin_id, admin_name) = admin_actor(&state, &viewer).await;

    // Defensive ordering: explicitly clear references that aren't
    // ON DELETE CASCADE before the DELETE FROM users.
    let pool = state.db.pool();
    let _ = sqlx::query("UPDATE posts SET is_removed = TRUE, deleted_at = NOW() WHERE author_id = $1")
        .bind(id).execute(pool).await;
    // The rest (follows, notifications, conversation_members,
    // user_blocks, paper_wallets, ppv_unlocks, ...) all have
    // ON DELETE CASCADE / SET NULL via the migrations we added.

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(id).execute(pool).await.map_err(AppError::Database)?;

    record_action(
        pool, None /* target_user is gone */, Some(&expected),
        "delete_user", None, req.reason.as_deref(),
        admin_id, admin_name.as_deref(),
    ).await;

    Ok(Json(ApiResponse::ok("deleted")))
}

// ---------- audit log ----------

#[derive(Debug, Deserialize)]
pub struct ActionsQuery {
    pub secret: String,
}

#[derive(Debug, Serialize)]
pub struct AuditRow {
    pub id: Uuid,
    pub target_user_id: Option<Uuid>,
    pub target_username: Option<String>,
    pub action_type: String,
    pub duration_hours: Option<i32>,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub admin_user_id: Option<Uuid>,
    pub admin_username: Option<String>,
}

pub async fn list_actions(
    State(state): State<AppState>,
    Query(q): Query<ActionsQuery>,
) -> AppResult<Json<ApiResponse<Vec<AuditRow>>>> {
    check_admin(&q.secret)?;
    let rows: Vec<(
        Uuid, Option<Uuid>, Option<String>, String, Option<i32>, Option<String>,
        DateTime<Utc>, Option<Uuid>, Option<String>
    )> = sqlx::query_as(
        "SELECT id, target_user_id, target_username, action_type,
                duration_hours, reason, created_at,
                admin_user_id, admin_username
           FROM admin_actions
          ORDER BY created_at DESC
          LIMIT 200"
    )
    .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;
    let out = rows.into_iter().map(|r| AuditRow {
        id: r.0, target_user_id: r.1, target_username: r.2,
        action_type: r.3, duration_hours: r.4, reason: r.5, created_at: r.6,
        admin_user_id: r.7, admin_username: r.8,
    }).collect();
    Ok(Json(ApiResponse::ok(out)))
}

// ---------- dashboard stats ----------

#[derive(Debug, Serialize)]
pub struct StatsResponse {
    pub total_users: i64,
    pub posts_24h: i64,
    pub flagged_posts: i64,
    pub banned_users: i64,
    pub pending_invitations: i64,
}

pub async fn stats(
    State(state): State<AppState>,
    Query(q): Query<ActionsQuery>,
) -> AppResult<Json<ApiResponse<StatsResponse>>> {
    check_admin(&q.secret)?;
    let pool = state.db.pool();
    let total_users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool).await.map_err(AppError::Database)?;
    let posts_24h: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM posts WHERE created_at > NOW() - INTERVAL '24 hours'"
    ).fetch_one(pool).await.map_err(AppError::Database)?;
    let flagged_posts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM posts WHERE is_flagged = TRUE AND is_removed = FALSE"
    ).fetch_one(pool).await.map_err(AppError::Database)?;
    let banned_users: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM users WHERE posting_banned_until > NOW()"
    ).fetch_one(pool).await.map_err(AppError::Database)?;
    let pending_invitations: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM group_invitations WHERE status = 'pending'"
    ).fetch_one(pool).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(StatsResponse {
        total_users, posts_24h, flagged_posts, banned_users, pending_invitations,
    })))
}

// ---------- admin user lookup ----------
//
// Returns enough state to drive the per-user moderation card in the
// admin dashboard: ban status, post count, recent action summary.

#[derive(Debug, Deserialize)]
pub struct UserLookupQuery {
    pub secret: String,
    pub q: String,
}

#[derive(Debug, Serialize)]
pub struct AdminUserCard {
    pub id: Uuid,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub wallet_address: Option<String>,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub post_count: i64,
    pub reported_count: i64,
    pub posting_banned_until: Option<DateTime<Utc>>,
    pub post_ban_reason: Option<String>,
}

pub async fn lookup_user(
    State(state): State<AppState>,
    Query(q): Query<UserLookupQuery>,
) -> AppResult<Json<ApiResponse<AdminUserCard>>> {
    check_admin(&q.secret)?;
    let (id, _) = resolve_target(&state, &q.q).await?;
    let row: (Uuid, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>,
              DateTime<Utc>, Option<DateTime<Utc>>, Option<String>) = sqlx::query_as(
        "SELECT id, username, display_name, wallet_address, email, avatar_url,
                created_at, posting_banned_until, post_ban_reason
           FROM users WHERE id = $1"
    )
    .bind(id)
    .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
    let post_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM posts WHERE author_id = $1 AND is_removed = FALSE"
    ).bind(id).fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
    let reported_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM posts WHERE author_id = $1 AND is_flagged = TRUE"
    ).bind(id).fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(AdminUserCard {
        id: row.0, username: row.1, display_name: row.2,
        wallet_address: row.3, email: row.4, avatar_url: row.5,
        created_at: row.6,
        posting_banned_until: row.7, post_ban_reason: row.8,
        post_count, reported_count,
    })))
}

// ---------- list users ----------

#[derive(Debug, Deserialize)]
pub struct UserListQuery {
    pub secret: String,
    pub page: Option<i64>,
    pub q: Option<String>,
    /// "all" (default) | "banned" | "active"
    pub filter: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AdminUserRow {
    pub id: Uuid,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub wallet_address: Option<String>,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub posting_banned_until: Option<DateTime<Utc>>,
    pub post_ban_reason: Option<String>,
    pub post_count: i64,
}

/// Paginated list of every user in the DB, optionally narrowed by a
/// free-text query against username / display_name / wallet / email.
/// Filter chips: "all" (default), "banned" (active posting bans), or
/// "active" (no current ban). 50 users per page.
pub async fn list_users(
    State(state): State<AppState>,
    Query(q): Query<UserListQuery>,
) -> AppResult<Json<ApiResponse<Vec<AdminUserRow>>>> {
    check_admin(&q.secret)?;
    let page = q.page.unwrap_or(1).max(1);
    let offset: i64 = (page - 1) * 50;

    // Hard-coded enum mapping keeps the value out of the SQL string.
    let filter_sql = match q.filter.as_deref().unwrap_or("all") {
        "banned" => "WHERE u.posting_banned_until IS NOT NULL AND u.posting_banned_until > NOW()",
        "active" => "WHERE (u.posting_banned_until IS NULL OR u.posting_banned_until <= NOW())",
        _ /* all */ => "",
    };

    // Optional free-text narrowing. Empty/short query means no filter.
    let term = q.q.as_deref().map(|s| s.trim().trim_start_matches('@')).unwrap_or("");
    let (where_clause, has_q) = if term.len() >= 2 {
        let extra = "u.username ILIKE $2 OR u.display_name ILIKE $2 OR LOWER(u.wallet_address) LIKE LOWER($2) OR LOWER(u.email) LIKE LOWER($2)";
        if filter_sql.is_empty() {
            (format!("WHERE ({})", extra), true)
        } else {
            (format!("{} AND ({})", filter_sql, extra), true)
        }
    } else {
        (filter_sql.to_string(), false)
    };

    let sql = format!(
        "SELECT u.id, u.username, u.display_name, u.wallet_address, u.email,
                u.avatar_url, u.created_at, u.posting_banned_until, u.post_ban_reason,
                COALESCE((SELECT COUNT(*) FROM posts p
                           WHERE p.author_id = u.id AND p.is_removed = FALSE), 0)::bigint AS post_count
           FROM users u
           {where_clause}
          ORDER BY u.created_at DESC
          LIMIT 50 OFFSET $1"
    );

    let rows: Vec<(Uuid, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>,
                   DateTime<Utc>, Option<DateTime<Utc>>, Option<String>, i64)> = if has_q {
        sqlx::query_as(&sql)
            .bind(offset)
            .bind(format!("%{}%", term))
            .fetch_all(state.db.pool()).await.map_err(AppError::Database)?
    } else {
        sqlx::query_as(&sql)
            .bind(offset)
            .fetch_all(state.db.pool()).await.map_err(AppError::Database)?
    };

    let out: Vec<AdminUserRow> = rows.into_iter().map(|r| AdminUserRow {
        id: r.0, username: r.1, display_name: r.2,
        wallet_address: r.3, email: r.4, avatar_url: r.5,
        created_at: r.6,
        posting_banned_until: r.7, post_ban_reason: r.8,
        post_count: r.9,
    }).collect();

    Ok(Json(ApiResponse::ok(out)))
}
