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

/// Resolve the caller's user id from either an email-auth subject
/// ("email:<uuid>") or a wallet address (looked up in `users`). The old
/// version parsed a wallet `0x...` address as a UUID, which always failed —
/// so wallet users got a 401 from update_post_visibility / repost_post.
async fn resolve_caller_id(state: &AppState, auth: &AuthUser) -> Result<Uuid, AppError> {
    if let Some(uuid_str) = auth.address.strip_prefix("email:") {
        uuid_str.parse::<Uuid>()
            .map_err(|_| AppError::Unauthorised("Invalid user ID".into()))
    } else {
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE wallet_address = $1")
            .bind(&auth.address)
            .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
            .ok_or_else(|| AppError::Unauthorised("User not found".into()))
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

    let user_id = resolve_caller_id(&state, &auth).await?;

    let result = sqlx::query(
        // Cast the bound text to the post_visibility ENUM — a plain text
        // parameter can't be assigned to an enum column without the cast.
        "UPDATE posts SET visibility = $1::post_visibility
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
    let user_id = resolve_caller_id(&state, &auth).await?;

    // Block reposting a repost — only original posts can be reposted
    let is_repost: Option<Uuid> = sqlx::query_scalar(
        "SELECT reposted_from FROM posts WHERE id = $1"
    )
    .bind(post_id)
    .fetch_optional(state.db.pool())
    .await
    .map_err(AppError::Database)?
    .flatten();

    if is_repost.is_some() {
        return Err(AppError::Conflict("Reposts cannot be reposted — only original posts".into()));
    }

    // Check user hasn't already reposted this post
    let already: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM posts WHERE reposted_from = $1 AND author_id = $2"
    )
    .bind(post_id)
    .bind(user_id)
    .fetch_one(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    if already > 0 {
        return Err(AppError::Conflict("Du hast diesen Post bereits geteilt (max. 1 Repost)".into()));
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

    // Notify the original author someone reshared. Best-effort.
    if let Some(author_id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT author_id FROM posts WHERE id = $1"
    ).bind(post_id).fetch_optional(state.db.pool()).await.ok().flatten() {
        let actor = sqlx::query_scalar::<_, Option<String>>(
            "SELECT COALESCE(display_name, username) FROM users WHERE id = $1"
        ).bind(user_id).fetch_optional(state.db.pool()).await
         .ok().flatten().flatten().unwrap_or_else(|| "Someone".into());
        crate::api::notifications::notify(
            state.db.pool(), author_id, Some(user_id),
            "reshare", &format!("{} reshared your post", actor), Some(post_id),
        ).await;
    }

    Ok(Json(ApiResponse::ok(SimpleOk { success: true })))
}

/// GET /api/v1/profile/:user_id/permanent
///
/// Login required: permanent posts are gated like the rest of the
/// feed, so anonymous visitors get a 401 instead of seeing content.
pub async fn get_permanent_posts(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    auth: AuthUser,
) -> AppResult<Json<serde_json::Value>> {
    // Resolve the viewer's UUID from either an email-auth subject
    // ("email:<uuid>") or a wallet address (looked up in users). The
    // previous code parsed a wallet address as a UUID, which always
    // failed — so a wallet user was never recognised as the owner of
    // their own permanent posts.
    let viewer_id: Option<Uuid> = if let Some(uuid_str) = auth.address.strip_prefix("email:") {
        uuid_str.parse::<Uuid>().ok()
    } else {
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE wallet_address = $1")
            .bind(&auth.address)
            .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
    };

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

    // Hide 18+ posts unless the viewer has completed age verification (owner always sees own).
    let viewer_age_verified: bool = if is_owner {
        true
    } else if let Some(vid) = viewer_id {
        sqlx::query_scalar::<_, bool>("SELECT age_verified_at IS NOT NULL FROM users WHERE id = $1")
            .bind(vid).fetch_optional(state.db.pool()).await.ok().flatten().unwrap_or(false)
    } else { false };

    let posts = sqlx::query_as::<_, (Uuid, String, Option<String>, String, chrono::DateTime<chrono::Utc>, i64, Option<i32>)>(
        // visibility is the post_visibility ENUM — cast to text or sqlx
        // can't decode it into a Rust String (was a 500 whenever the list
        // was non-empty).
        "SELECT id, content, media_url, COALESCE(visibility::text, 'public'), created_at, like_count, repost_count
         FROM posts WHERE author_id = $1 AND is_permanent = TRUE AND is_removed = FALSE AND deleted_at IS NULL
         AND ($2 = TRUE OR COALESCE(visibility::text, 'public') = 'public')
         AND ($3 = TRUE OR is_adult = FALSE)
         ORDER BY created_at DESC"
    )
    .bind(user_id)
    .bind(can_see_followers)
    .bind(viewer_age_verified)
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

/// GET /api/v1/me/permanent
///
/// Owner-only view that bypasses the cached `yeet_user_id` in the
/// frontend (which was sometimes stale or absent and silently returned
/// 0 rows). The caller is the author by construction, so visibility +
/// age-filter gates collapse to "always show".
pub async fn get_my_permanent_posts(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<serde_json::Value>> {
    let user_id: Uuid = if let Some(uuid_str) = auth.address.strip_prefix("email:") {
        uuid_str.parse::<Uuid>()
            .map_err(|_| AppError::Unauthorised("Invalid user ID".into()))?
    } else {
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE wallet_address = $1")
            .bind(&auth.address)
            .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
            .ok_or_else(|| AppError::NotFound("User not found".into()))?
    };

    let posts = sqlx::query_as::<_, (Uuid, String, Option<String>, String, chrono::DateTime<chrono::Utc>, i64, Option<i32>)>(
        // visibility is the post_visibility ENUM — cast to text (see above).
        "SELECT id, content, media_url, COALESCE(visibility::text, 'public'), created_at, like_count, repost_count
         FROM posts
         WHERE author_id = $1 AND is_permanent = TRUE
           AND is_removed = FALSE AND deleted_at IS NULL
         ORDER BY created_at DESC"
    )
    .bind(user_id)
    .fetch_all(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    let result: Vec<serde_json::Value> = posts.iter().map(|p| serde_json::json!({
        "id": p.0, "content": p.1, "media_url": p.2,
        "visibility": p.3, "created_at": p.4,
        "like_count": p.5, "repost_count": p.6, "is_permanent": true,
        "author": { "id": user_id },
    })).collect();

    Ok(Json(serde_json::json!({ "success": true, "data": result, "owner_id": user_id })))
}
