//! Notification handlers + insertion helpers.
//!
//! `notify(...)` is the single insertion point other modules call when
//! an event happens that the recipient should hear about (someone liked
//! their post, started following them, tipped them, etc.). It is
//! intentionally cheap and best-effort — if it fails we log and move
//! on; we never want a follow/like/tip to roll back because a
//! notification couldn't be written.

use axum::{extract::{Path, State}, Json};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;
use chrono::{DateTime, Utc};

use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;

#[derive(Debug, Serialize)]
pub struct Notification {
    pub id: Uuid,
    pub notification_type: String,
    pub message: String,
    pub from_address: Option<String>,
    pub from_username: Option<String>,
    pub from_avatar_url: Option<String>,
    pub from_user_id: Option<Uuid>,
    pub post_id: Option<Uuid>,
    pub read: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct UnreadCountResponse {
    pub count: i64,
}

async fn caller_user_id(state: &AppState, auth: &AuthUser) -> AppResult<Uuid> {
    if let Some(rest) = auth.address.strip_prefix("email:") {
        return Uuid::parse_str(rest)
            .map_err(|_| AppError::Validation("Invalid user id".into()));
    }
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE wallet_address = $1")
        .bind(&auth.address)
        .fetch_optional(state.db.pool())
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))
}

/// Insert a notification row. Best-effort: errors are logged but never
/// propagated so the caller's primary action (follow, like, tip, ...)
/// doesn't roll back on a notify failure. Self-notifications are
/// silently skipped.
pub async fn notify(
    pool: &PgPool,
    user_id: Uuid,
    from_user_id: Option<Uuid>,
    notification_type: &str,
    message: &str,
    post_id: Option<Uuid>,
) {
    if Some(user_id) == from_user_id { return; }
    let res = sqlx::query(
        "INSERT INTO notifications (user_id, from_user_id, notification_type, message, post_id)
         VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(user_id)
    .bind(from_user_id)
    .bind(notification_type)
    .bind(message)
    .bind(post_id)
    .execute(pool)
    .await;
    if let Err(e) = res {
        tracing::warn!(error = %e, "notify insert failed");
    }
}

pub async fn get_notifications(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<Vec<Notification>>>> {
    let me = caller_user_id(&state, &auth).await?;
    let rows: Vec<(Uuid, String, String, Option<Uuid>, Option<String>, Option<String>, Option<String>, Option<Uuid>, bool, DateTime<Utc>)> =
        sqlx::query_as(
            "SELECT n.id, n.notification_type, n.message,
                    n.from_user_id, u.wallet_address, u.username, u.avatar_url,
                    n.post_id, n.read, n.created_at
               FROM notifications n
               LEFT JOIN users u ON n.from_user_id = u.id
              WHERE n.user_id = $1
              ORDER BY n.created_at DESC
              LIMIT 50"
        )
        .bind(me)
        .fetch_all(state.db.pool())
        .await
        .map_err(AppError::Database)?;

    let out: Vec<Notification> = rows.into_iter().map(|r| Notification {
        id: r.0, notification_type: r.1, message: r.2,
        from_user_id: r.3, from_address: r.4, from_username: r.5, from_avatar_url: r.6,
        post_id: r.7, read: r.8, created_at: r.9,
    }).collect();
    Ok(Json(ApiResponse::ok(out)))
}

pub async fn unread_count(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<UnreadCountResponse>>> {
    let me = caller_user_id(&state, &auth).await?;
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE user_id = $1 AND read = false"
    )
    .bind(me)
    .fetch_one(state.db.pool())
    .await
    .map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(UnreadCountResponse { count: n })))
}

pub async fn mark_notifications_read(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<serde_json::Value>>> {
    let me = caller_user_id(&state, &auth).await?;
    sqlx::query(
        "UPDATE notifications SET read = true WHERE user_id = $1 AND read = false"
    )
    .bind(me)
    .execute(state.db.pool())
    .await
    .map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(serde_json::json!({"updated": true}))))
}

pub async fn mark_one_read(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    let me = caller_user_id(&state, &auth).await?;
    sqlx::query(
        "UPDATE notifications SET read = true WHERE id = $1 AND user_id = $2"
    )
    .bind(id).bind(me)
    .execute(state.db.pool())
    .await
    .map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok("ok")))
}
