use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::{AppError, AppResult},
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct VisibilityUpdate {
    pub visibility: String, // "public" or "followers"
}

#[derive(Debug, Serialize)]
pub struct SimpleResponse {
    pub success: bool,
}

/// PATCH /api/v1/posts/:id/visibility
/// Only the post owner can change visibility of their permanent posts
pub async fn update_post_visibility(
    State(state): State<AppState>,
    Path(post_id): Path<Uuid>,
    auth: AuthUser,
    Json(req): Json<VisibilityUpdate>,
) -> AppResult<Json<SimpleResponse>> {
    if req.visibility != "public" && req.visibility != "followers" {
        return Err(AppError::BadRequest("visibility must be 'public' or 'followers'".into()));
    }

    let result = sqlx::query(
        "UPDATE posts SET visibility = $1
         WHERE id = $2 AND author_id = $3 AND is_permanent = TRUE"
    )
    .bind(&req.visibility)
    .bind(post_id)
    .bind(auth.user_id)
    .execute(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Post not found or not a permanent post you own".into()));
    }

    Ok(Json(SimpleResponse { success: true }))
}

/// POST /api/v1/posts/:id/repost
/// Repost a post — max 1 repost per user per post
pub async fn repost_post(
    State(state): State<AppState>,
    Path(post_id): Path<Uuid>,
    auth: AuthUser,
) -> AppResult<Json<SimpleResponse>> {
    // Check original post exists and user hasn't already reposted
    let original = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM posts WHERE reposted_from = $1 AND author_id = $2"
    )
    .bind(post_id)
    .bind(auth.user_id)
    .fetch_one(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    if original > 0 {
        return Err(AppError::BadRequest("You have already reposted this post".into()));
    }

    // Get original post content
    let orig = sqlx::query_as::<_, (Uuid, String, Option<String>)>(
        "SELECT id, content, media_url FROM posts WHERE id = $1 AND is_removed = FALSE"
    )
    .bind(post_id)
    .fetch_optional(state.db.pool())
    .await
    .map_err(AppError::Database)?
    .ok_or_else(|| AppError::NotFound("Post not found".into()))?;

    let expires_at = chrono::Utc::now() + chrono::Duration::hours(24);

    // Create repost
    sqlx::query(
        "INSERT INTO posts (author_id, content, media_url, reposted_from, expires_at)
         VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(auth.user_id)
    .bind(&orig.1)
    .bind(&orig.2)
    .bind(orig.0)
    .bind(expires_at)
    .execute(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    // Increment repost count on original
    sqlx::query("UPDATE posts SET repost_count = repost_count + 1 WHERE id = $1")
        .bind(post_id)
        .execute(state.db.pool())
        .await
        .map_err(AppError::Database)?;

    Ok(Json(SimpleResponse { success: true }))
}

/// GET /api/v1/profile/:user_id/permanent
/// Returns user's permanent posts — respects visibility rules
pub async fn get_permanent_posts(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    auth: Option<AuthUser>,
) -> AppResult<Json<serde_json::Value>> {
    let viewer_id = auth.map(|a| a.user_id);
    let is_owner = viewer_id == Some(user_id);

    let posts = if is_owner {
        // Owner sees all their permanent posts
        sqlx::query_as::<_, (Uuid, String, Option<String>, String, chrono::DateTime<chrono::Utc>, i32, i32)>(
            "SELECT id, content, media_url, visibility, created_at, repost_count, like_count
             FROM posts WHERE author_id = $1 AND is_permanent = TRUE AND is_removed = FALSE
             ORDER BY created_at DESC"
        )
        .bind(user_id)
        .fetch_all(state.db.pool())
        .await
        .map_err(AppError::Database)?
    } else {
        // Check if viewer follows the user
        let is_follower = if let Some(vid) = viewer_id {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM follows WHERE follower_id = $1 AND following_id = $2"
            )
            .bind(vid).bind(user_id)
            .fetch_one(state.db.pool()).await.unwrap_or(0) > 0
        } else { false };

        sqlx::query_as::<_, (Uuid, String, Option<String>, String, chrono::DateTime<chrono::Utc>, i32, i32)>(
            "SELECT id, content, media_url, visibility, created_at, repost_count, like_count
             FROM posts WHERE author_id = $1 AND is_permanent = TRUE AND is_removed = FALSE
             AND ($2 = TRUE OR visibility = 'public')
             ORDER BY created_at DESC"
        )
        .bind(user_id)
        .bind(is_follower)
        .fetch_all(state.db.pool())
        .await
        .map_err(AppError::Database)?
    };

    let result: Vec<serde_json::Value> = posts.iter().map(|p| serde_json::json!({
        "id": p.0,
        "content": p.1,
        "media_url": p.2,
        "visibility": p.3,
        "created_at": p.4,
        "repost_count": p.5,
        "like_count": p.6,
        "is_permanent": true,
    })).collect();

    Ok(Json(serde_json::json!({ "success": true, "data": result })))
}
