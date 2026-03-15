//! Post CRUD, likes, reshares, comments, NFT minting.
use axum::{extract::{Path, State}, Json};
use chrono::{Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, models::{ApiResponse, Comment, FeedPost, FeedPostAuthor}};
use crate::api::middleware::AuthUser;
use crate::services::tokens::{self, rewards, RewardAction};

#[derive(Debug, Deserialize)]
pub struct CreatePostRequest {
    pub content: String,
    pub media_url: Option<String>,
    pub is_adult: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct AddCommentRequest { pub content: String }

/// POST /api/v1/posts
pub async fn create_post(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreatePostRequest>,
) -> AppResult<Json<ApiResponse<Uuid>>> {
    if req.content.trim().is_empty() || req.content.len() > 280 {
        return Err(AppError::Validation("Post content must be 1-280 characters".into()));
    }

    let user = sqlx::query!("SELECT id FROM users WHERE wallet_address = $1", auth.address)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))?;

    let expires_at = Utc::now() + ChronoDuration::hours(24);

    let post_id = sqlx::query_scalar!(
        r#"INSERT INTO posts (author_id, content, media_url, is_adult, expires_at)
           VALUES ($1, $2, $3, $4, $5) RETURNING id"#,
        user.id, req.content, req.media_url, req.is_adult.unwrap_or(false), expires_at
    )
    .fetch_one(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    // Grant reward (fire-and-forget — do not fail request on reward error)
    let _ = tokens::grant_reward(&state.db, user.id, RewardAction::PostCreated, rewards::POST_CREATED).await;

    Ok(Json(ApiResponse::ok(post_id)))
}

/// GET /api/v1/posts/:id
pub async fn get_post(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<FeedPost>>> {
    let r = sqlx::query!(
        r#"SELECT p.id, p.content, p.media_url, p.is_adult, p.nft_token_id,
                  p.like_count, p.reshare_count, p.comment_count, p.expires_at, p.created_at,
                  u.id as author_id, u.wallet_address, u.display_name, u.avatar_url
           FROM posts p JOIN users u ON p.author_id = u.id
           WHERE p.id = $1 AND p.expires_at > NOW()"#,
        id
    )
    .fetch_optional(state.db.pool())
    .await
    .map_err(AppError::Database)?
    .ok_or_else(|| AppError::NotFound("Post not found".into()))?;

    Ok(Json(ApiResponse::ok(FeedPost {
        id: r.id, content: r.content, media_url: r.media_url, is_adult: r.is_adult,
        is_nft: r.nft_token_id.is_some(), like_count: r.like_count, reshare_count: r.reshare_count,
        comment_count: r.comment_count, is_liked: false, expires_at: r.expires_at, created_at: r.created_at,
        author: FeedPostAuthor { id: r.author_id, wallet_address: r.wallet_address, display_name: r.display_name, avatar_url: r.avatar_url },
    })))
}

/// DELETE /api/v1/posts/:id
pub async fn delete_post(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<()>>> {
    let user = sqlx::query!("SELECT id FROM users WHERE wallet_address = $1", auth.address)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))?;

    let result = sqlx::query!(
        "DELETE FROM posts WHERE id = $1 AND author_id = $2 AND nft_token_id IS NULL",
        id, user.id
    )
    .execute(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Post not found or cannot be deleted (NFT posts are permanent)".into()));
    }

    Ok(Json(ApiResponse::ok(())))
}

/// POST /api/v1/posts/:id/like
pub async fn like_post(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<()>>> {
    let user = sqlx::query!("SELECT id FROM users WHERE wallet_address = $1", auth.address)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))?;

    // Insert like (idempotent via ON CONFLICT DO NOTHING)
    let result = sqlx::query!(
        "INSERT INTO post_likes (post_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        id, user.id
    )
    .execute(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    if result.rows_affected() > 0 {
        sqlx::query!("UPDATE posts SET like_count = like_count + 1 WHERE id = $1", id)
            .execute(state.db.pool()).await.map_err(AppError::Database)?;
    }

    Ok(Json(ApiResponse::ok(())))
}

/// POST /api/v1/posts/:id/reshare — resets 24h expiry timer
pub async fn reshare_post(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<()>>> {
    let user = sqlx::query!("SELECT id FROM users WHERE wallet_address = $1", auth.address)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))?;

    let new_expiry = Utc::now() + ChronoDuration::hours(24);

    sqlx::query!(
        "UPDATE posts SET reshare_count = reshare_count + 1, expires_at = GREATEST(expires_at, $1) WHERE id = $2",
        new_expiry, id
    )
    .execute(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    let _ = tokens::grant_reward(&state.db, user.id, RewardAction::PostReshared, rewards::POST_RESHARED).await;

    Ok(Json(ApiResponse::ok(())))
}

/// GET /api/v1/posts/:id/comments
pub async fn get_comments(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<Vec<Comment>>>> {
    let comments = sqlx::query_as!(
        Comment,
        "SELECT id, post_id, author_id, content, created_at FROM comments WHERE post_id = $1 ORDER BY created_at ASC",
        id
    )
    .fetch_all(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(comments)))
}

/// POST /api/v1/posts/:id/comments
pub async fn add_comment(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<AddCommentRequest>,
) -> AppResult<Json<ApiResponse<Uuid>>> {
    if req.content.trim().is_empty() || req.content.len() > 280 {
        return Err(AppError::Validation("Comment must be 1-280 characters".into()));
    }

    let user = sqlx::query!("SELECT id FROM users WHERE wallet_address = $1", auth.address)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))?;

    let comment_id = sqlx::query_scalar!(
        "INSERT INTO comments (post_id, author_id, content) VALUES ($1, $2, $3) RETURNING id",
        id, user.id, req.content
    )
    .fetch_one(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    sqlx::query!("UPDATE posts SET comment_count = comment_count + 1 WHERE id = $1", id)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;

    let _ = tokens::grant_reward(&state.db, user.id, RewardAction::CommentPosted, rewards::COMMENT_POSTED).await;

    Ok(Json(ApiResponse::ok(comment_id)))
}

/// POST /api/v1/posts/:id/nft — mint post as NFT (5 YEET burn fee)
pub async fn mint_nft(
    State(_state): State<AppState>,
    _auth: AuthUser,
    Path(_id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<String>>> {
    // TODO: implement NFT minting via ethers-rs once contracts are deployed
    Err(AppError::Internal("NFT minting not yet available — contracts not deployed".into()))
}
