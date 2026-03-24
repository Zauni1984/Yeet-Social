//! Notification handlers.
use axum::{extract::State, Json};
use serde::Serialize;
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;
use chrono::{DateTime, Utc};

#[derive(Debug, Serialize)]
pub struct Notification {
    pub id: Uuid,
    pub notification_type: String,
    pub message: String,
    pub from_address: Option<String>,
    pub post_id: Option<Uuid>,
    pub read: bool,
    pub created_at: DateTime<Utc>,
}

pub async fn get_notifications(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<Vec<Notification>>>> {
    let rows = sqlx::query!(
        r#"SELECT
            n.id, n.notification_type, n.message,
            u.wallet_address as from_address,
            n.post_id, n.read, n.created_at
           FROM notifications n
           LEFT JOIN users u ON n.from_user_id = u.id
           WHERE n.user_id = (SELECT id FROM users WHERE wallet_address = $1)
           ORDER BY n.created_at DESC
           LIMIT 50"#,
        auth.address
    )
    .fetch_all(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    let notifications: Vec<Notification> = rows.into_iter().map(|r| Notification {
        id: r.id,
        notification_type: r.notification_type,
        message: r.message,
        from_address: r.from_address,
        post_id: r.post_id,
        read: r.read,
        created_at: r.created_at,
    }).collect();

    Ok(Json(ApiResponse::ok(notifications)))
}

pub async fn mark_notifications_read(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<()>>> {
    sqlx::query!(
        "UPDATE notifications SET read = true
         WHERE user_id = (SELECT id FROM users WHERE wallet_address = $1)
         AND read = false",
        auth.address
    )
    .execute(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(())))
}
