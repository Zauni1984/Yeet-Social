//! Permanent post visibility + repost handlers.
use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;

#[derive(Debug, Deserialize)]
pub struct VisibilityUpdate {
    pub visibility: String,
}

#[derive(Debug, Serialize)]
pub struct SimpleOk {
    pub success: bool,
}

fn parse_user_id(auth: &AuthUser) -> Result<Uuid, AppError> {
    if let Some(uuid_str) = auth.address.strip_prefix("email:") {
        uuid_str.parse::<Uuid>()
            .map_err(|_| AppError::Unauthorised("Invalid user ID".into()))
    } else {
        auth.address.parse::<Uuid>()
            .map_err(|_| AppError::Unauthorised("Invalid address".into()))
    }
}

/// PATCH /api/v1/posts/:id/visibility
pub async fn update_post_visibility(
    State(state): State<AppState>,
    Path(post_id): Path<Uuid>,
    auth: AuthUser,
    Json(req): Json<VisibilityUpdate>,
) -> AppResult<Json<ApiResponse<SimpleOk>>> {
    if req.visibility != "public" && req.visibility != "followers" {
        return Err(AppError::Validation("visibility must be 'public' or 'followers'".into()));
    }

    let user_id = parse_user_id(&auth)?;

    let result = sqlx::query(
        "UPDATE posts SET visibility = $1
         WHERE id = $2 AND author_id = $3 AND is_permanent = TRUE"
    )
    .bind(&req.visibility)
    .bind(post_id)
    .bind(user_id)
    .execute(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Post not found or not your permanent post".into()));
    }

    Ok(Json(ApiResponse::ok(SimpleOk { success: true })))
}

/// POST /api/v1/posts/:id/repost — max 1 repost per user per post
pub async fn repost_post(
    State(state): State<AppState>,
    Path(post_id): Path<Uuid>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<SimpleOk>>> {
    let user_id = parse_user_id(&auth)?;

    // Check already reposted
    let already: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM posts WHERE reposted_from = $1 AND author_id = $2"
    )
    .bind(post_id)
    .bind(user_id)
    .fetch_one(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    if already > 0 {
        return Err(AppError::Conflict("Already reposted this post".into()));
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

    sqlx::query(
        "INSERT INTO posts (author_id, content, media_url, reposted_from, expires_at)
         VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(user_id)
    .bind(&orig.1)
    .bind(&orig.2)
    .bind(orig.0)
    .bind(expires_at)
    .execute(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    // Increment repost count on original
    let _ = sqlx::query("UPDATE posts SET repost_count = repost_count + 1 WHERE id = $1")
        .bind(post_id)
        .execute(state.db.pool())
        .await;

    Ok(Json(ApiResponse::ok(SimpleOk { success: true })))
}

/// GET /api/v1/profile/:user_id/permanent
pub async fn get_permanent_posts(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    auth: Option<AuthUser>,
) -> AppResult<Json<serde_json::Value>> {
    let viewer_id: Option<Uuid> = auth.and_then(|a| {
        if let Some(uuid_str) = a.address.strip_prefix("email:") {
            uuid_str.parse::<Uuid>().ok()
        } else {
            a.address.parse::<Uuid>().ok()
        }
    });

    let is_owner = viewer_id == Some(user_id);

    let can_see_followers = if is_owner {
        true
    } else if let Some(vid) = viewer_id {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM follows WHERE follower_id = $1 AND following_id = $2"
        )
        .bind(vid).bind(user_id)
        .fetch_one(state.db.pool()).await.unwrap_or(0) > 0
    } else { false };

    let posts = sqlx::query_as::<_, (Uuid, String, Option<String>, String, chrono::DateTime<chrono::Utc>, i64, Option<i32>)>(
        "SELECT id, content, media_url, COALESCE(visibility, 'public'), created_at, like_count, repost_count
         FROM posts WHERE author_id = $1 AND is_permanent = TRUE AND is_removed = FALSE
         AND ($2 = TRUE OR COALESCE(visibility, 'public') = 'public')
         ORDER BY created_at DESC"
    )
    .bind(user_id)
    .bind(can_see_followers)
    .fetch_all(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    let result: Vec<serde_json::Value> = posts.iter().map(|p| serde_json::json!({
        "id": p.0, "content": p.1, "media_url": p.2,
        "visibility": p.3, "created_at": p.4,
        "like_count": p.5, "repost_count": p.6, "is_permanent": true,
    })).collect();

    Ok(Json(serde_json::json!({ "success": true, "data": result })))
}
