//! Scheduled posts — staging area for future publication.
//!
//! Rows live in `scheduled_posts` until their `publish_at` is due, at
//! which point the worker in `services/batch_rewards.rs` moves them
//! into `posts` (with `expires_at = publish_at + 24h`, keeping the
//! ephemeral rule intact). Pending rows are invisible to feed queries
//! because they're in a separate table — no `WHERE publish_at <= NOW()`
//! filter needs to be retro-fitted onto every existing query.
use axum::{extract::{Path, State}, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;

async fn resolve_user_id(state: &AppState, auth_address: &str) -> AppResult<Uuid> {
    if let Some(uuid_str) = auth_address.strip_prefix("email:") {
        return uuid_str.parse::<Uuid>()
            .map_err(|_| AppError::NotFound("Invalid user ID".into()));
    }
    sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
        .bind(auth_address)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))
}

#[derive(Debug, Deserialize)]
pub struct ScheduleRequest {
    pub content: String,
    pub media_url: Option<String>,
    pub is_adult: Option<bool>,
    pub is_nft: Option<bool>,
    pub nft_price_yeet: Option<f64>,
    pub is_permanent: Option<bool>,
    pub ppv_price_yeet: Option<f64>,
    pub publish_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ScheduledPost {
    pub id: Uuid,
    pub content: String,
    pub media_url: Option<String>,
    pub is_adult: bool,
    pub is_nft: bool,
    pub nft_price_yeet: Option<f64>,
    pub is_permanent: bool,
    pub ppv_price_yeet: Option<f64>,
    pub publish_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<ScheduleRequest>,
) -> AppResult<Json<ApiResponse<Uuid>>> {
    let content = req.content.trim();
    if content.is_empty() || content.chars().count() > 280 {
        return Err(AppError::Validation("Post content must be 1-280 chars".into()));
    }
    // Minimum 60s in the future to avoid an instant-publish trick that
    // bypasses the worker's batching window.
    if req.publish_at < Utc::now() + chrono::Duration::seconds(60) {
        return Err(AppError::Validation("publish_at must be at least 1 minute in the future".into()));
    }
    if req.publish_at > Utc::now() + chrono::Duration::days(60) {
        return Err(AppError::Validation("Cannot schedule more than 60 days ahead".into()));
    }
    let user_id = resolve_user_id(&state, &auth.address).await?;

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO scheduled_posts
           (author_id, content, media_url, is_adult, is_nft, nft_price_yeet,
            is_permanent, ppv_price_yeet, publish_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING id"
    )
    .bind(user_id).bind(content).bind(&req.media_url)
    .bind(req.is_adult.unwrap_or(false))
    .bind(req.is_nft.unwrap_or(false))
    .bind(req.nft_price_yeet)
    // Mirror create_post: an NFT scheduled post is permanent too, so the
    // published row lands in the permanent list (not just the 24h feed).
    .bind(req.is_permanent.unwrap_or(false) || req.is_nft.unwrap_or(false))
    .bind(req.ppv_price_yeet)
    .bind(req.publish_at)
    .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(id)))
}

pub async fn list_mine(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<Vec<ScheduledPost>>>> {
    let user_id = resolve_user_id(&state, &auth.address).await?;
    let rows = sqlx::query_as::<_, ScheduledPost>(
        "SELECT id, content, media_url, is_adult, is_nft,
                nft_price_yeet::float8 AS nft_price_yeet,
                is_permanent,
                ppv_price_yeet::float8 AS ppv_price_yeet,
                publish_at, created_at
           FROM scheduled_posts
          WHERE author_id = $1
          ORDER BY publish_at ASC"
    )
    .bind(user_id)
    .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(rows)))
}

pub async fn cancel(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<()>>> {
    let user_id = resolve_user_id(&state, &auth.address).await?;
    let r = sqlx::query(
        "DELETE FROM scheduled_posts WHERE id = $1 AND author_id = $2"
    )
    .bind(id).bind(user_id)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound("Scheduled post not found".into()));
    }
    Ok(Json(ApiResponse::ok(())))
}
