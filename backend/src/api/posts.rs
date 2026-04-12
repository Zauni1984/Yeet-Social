#![allow(dead_code)]
//! Post CRUD, likes, reshares, comments.
use axum::{extract::{Path, State}, Json};
use chrono::{Duration as ChronoDuration, Utc, DateTime};
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
    pub is_nft: Option<bool>,
    pub nft_price_yeet: Option<f64>,
    pub is_permanent: Option<bool>,
    pub ppv_price_yeet: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct AddCommentRequest { pub content: String }

#[derive(sqlx::FromRow)]
struct PostRow {
    id: Uuid,
    content: String,
    media_urls: Option<Vec<String>>,
    is_nft: bool,
    nft_token_id: Option<String>,
    like_count: i64,
    reshare_count: i64,
    comment_count: i64,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    author_id: Uuid,
    wallet_address: String,
    display_name: Option<String>,
    avatar_url: Option<String>,
}

fn row_to_feed_post(r: PostRow) -> FeedPost {
    let media_url = r.media_urls.and_then(|v| v.into_iter().next());
    FeedPost {
        id: r.id, content: r.content, media_url, is_adult: false,
        is_nft: r.is_nft,
        like_count: r.like_count as i32,
        reshare_count: r.reshare_count as i32,
        comment_count: r.comment_count as i32,
        is_liked: false, expires_at: r.expires_at, created_at: r.created_at,
        author: FeedPostAuthor {
            id: r.author_id, wallet_address: Some(r.wallet_address),
            display_name: r.display_name, avatar_url: r.avatar_url,
        },
        tip_total_yeet: None,
        nft_price_yeet: None,
        is_permanent: false,
        ppv_price_yeet: None,
    }
}

pub async fn create_post(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreatePostRequest>,
) -> AppResult<Json<ApiResponse<Uuid>>> {
    if req.content.trim().is_empty() || req.content.len() > 280 {
        return Err(AppError::Validation("Post content must be 1-280 chars".into()));
    }
    // Support both wallet users (auth.address = "0x...") and email users (auth.address = "email:UUID")
    let user_id: Uuid = if let Some(uuid_str) = auth.address.strip_prefix("email:") {
        uuid_str.parse::<Uuid>().map_err(|_| AppError::NotFound("Invalid user ID".into()))?
    } else {
        sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
            .bind(&auth.address)
            .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
            .ok_or_else(|| AppError::NotFound("User not found".into()))?
    };

    let is_permanent = req.is_permanent.unwrap_or(false) || req.is_nft.unwrap_or(false);
    let expires_at = if is_permanent {
        Utc::now() + ChronoDuration::hours(24 * 365 * 100)
    } else {
        Utc::now() + ChronoDuration::hours(24)
    };
    let media_url_clone = req.media_url.clone();
    let media_arr: Vec<String> = req.media_url.into_iter().collect();
    let post_id: Uuid = sqlx::query_scalar(
        "INSERT INTO posts (author_id, content, media_urls, media_url, expires_at, is_adult, is_nft, nft_price_yeet, is_permanent, ppv_price_yeet)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING id"
    )
    .bind(user_id).bind(&req.content).bind(&media_arr)
    .bind(media_url_clone.as_deref())
    .bind(expires_at)
    .bind(req.is_adult.unwrap_or(false))
    .bind(req.is_nft.unwrap_or(false))
    .bind(req.nft_price_yeet)
    .bind(req.is_permanent.unwrap_or(false))
    .bind(req.ppv_price_yeet)
    .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;

    let _ = tokens::grant_reward(&state.db, user_id, RewardAction::PostCreated, rewards::POST_CREATED).await;
    Ok(Json(ApiResponse::ok(post_id)))
}

pub async fn get_post(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<FeedPost>>> {
    let r = sqlx::query_as::<_, PostRow>(
        "SELECT p.id, p.content, p.media_urls, p.is_nft, p.nft_token_id,
                p.like_count, p.reshare_count, p.comment_count, p.expires_at, p.created_at,
                u.id as author_id, u.wallet_address, u.display_name, u.avatar_url
         FROM posts p JOIN users u ON p.author_id = u.id
         WHERE p.id = $1 AND p.expires_at > NOW() AND p.deleted_at IS NULL"
    )
    .bind(id)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
    .ok_or_else(|| AppError::NotFound("Post not found".into()))?;
    Ok(Json(ApiResponse::ok(row_to_feed_post(r))))
}

pub async fn delete_post(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<()>>> {
    // Support both wallet users (auth.address = "0x...") and email users (auth.address = "email:UUID")
    let user_id: Uuid = if let Some(uuid_str) = auth.address.strip_prefix("email:") {
        uuid_str.parse::<Uuid>().map_err(|_| AppError::NotFound("Invalid user ID".into()))?
    } else {
        sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
            .bind(&auth.address)
            .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
            .ok_or_else(|| AppError::NotFound("User not found".into()))?
    };

    let result = sqlx::query(
        "UPDATE posts SET deleted_at = NOW() WHERE id = $1 AND author_id = $2 AND is_nft = false AND deleted_at IS NULL"
    )
    .bind(id).bind(user_id)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Post not found or cannot be deleted".into()));
    }
    Ok(Json(ApiResponse::ok(())))
}

pub async fn like_post(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<()>>> {
    // Support both wallet users (auth.address = "0x...") and email users (auth.address = "email:UUID")
    let user_id: Uuid = if let Some(uuid_str) = auth.address.strip_prefix("email:") {
        uuid_str.parse::<Uuid>().map_err(|_| AppError::NotFound("Invalid user ID".into()))?
    } else {
        sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
            .bind(&auth.address)
            .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
            .ok_or_else(|| AppError::NotFound("User not found".into()))?
    };

    let inserted = sqlx::query(
        "INSERT INTO post_likes (post_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING"
    )
    .bind(id).bind(user_id)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;

    if inserted.rows_affected() > 0 {
        sqlx::query("UPDATE posts SET like_count = like_count + 1 WHERE id = $1")
            .bind(id).execute(state.db.pool()).await.map_err(AppError::Database)?;
    }
    Ok(Json(ApiResponse::ok(())))
}

pub async fn unlike_post(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<()>>> {
    let user_id: Uuid = if let Some(uuid_str) = auth.address.strip_prefix("email:") {
        uuid_str.parse().map_err(|_| AppError::Unauthorised("Invalid user ID".into()))?
    } else {
        auth.user_id
    };

    sqlx::query(
        "DELETE FROM post_likes WHERE post_id = $1 AND user_id = $2"
    )
    .bind(id)
    .bind(user_id)
    .execute(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    sqlx::query("UPDATE posts SET like_count = GREATEST(0, like_count - 1) WHERE id = $1")
        .bind(id)
        .execute(state.db.pool())
        .await
        .map_err(AppError::Database)?;

    Ok(Json(ApiResponse::success(())))
}


pub async fn reshare_post(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<()>>> {
    // Support both wallet users (auth.address = "0x...") and email users (auth.address = "email:UUID")
    let user_id: Uuid = if let Some(uuid_str) = auth.address.strip_prefix("email:") {
        uuid_str.parse::<Uuid>().map_err(|_| AppError::NotFound("Invalid user ID".into()))?
    } else {
        sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
            .bind(&auth.address)
            .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
            .ok_or_else(|| AppError::NotFound("User not found".into()))?
    };

    let new_expiry = Utc::now() + ChronoDuration::hours(24);
    sqlx::query(
        "UPDATE posts SET reshare_count = reshare_count + 1, expires_at = GREATEST(expires_at, $1) WHERE id = $2"
    )
    .bind(new_expiry).bind(id)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;

    let _ = tokens::grant_reward(&state.db, user_id, RewardAction::PostReshared, rewards::POST_RESHARED).await;
    Ok(Json(ApiResponse::ok(())))
}

pub async fn get_comments(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<Vec<Comment>>>> {
    let comments = sqlx::query_as::<_, Comment>(
        "SELECT id, post_id, author_id, content, created_at FROM comments WHERE post_id = $1 ORDER BY created_at ASC"
    )
    .bind(id)
    .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(comments)))
}

pub async fn add_comment(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<AddCommentRequest>,
) -> AppResult<Json<ApiResponse<Uuid>>> {
    if req.content.trim().is_empty() || req.content.len() > 280 {
        return Err(AppError::Validation("Comment must be 1-280 chars".into()));
    }
    // Support both wallet users (auth.address = "0x...") and email users (auth.address = "email:UUID")
    let user_id: Uuid = if let Some(uuid_str) = auth.address.strip_prefix("email:") {
        uuid_str.parse::<Uuid>().map_err(|_| AppError::NotFound("Invalid user ID".into()))?
    } else {
        sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
            .bind(&auth.address)
            .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
            .ok_or_else(|| AppError::NotFound("User not found".into()))?
    };

    let comment_id: Uuid = sqlx::query_scalar(
        "INSERT INTO comments (post_id, author_id, content) VALUES ($1, $2, $3) RETURNING id"
    )
    .bind(id).bind(user_id).bind(&req.content)
    .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;

    sqlx::query("UPDATE posts SET comment_count = comment_count + 1 WHERE id = $1")
        .bind(id).execute(state.db.pool()).await.map_err(AppError::Database)?;

    let _ = tokens::grant_reward(&state.db, user_id, RewardAction::CommentPosted, rewards::COMMENT_POSTED).await;
    Ok(Json(ApiResponse::ok(comment_id)))
}

pub async fn mint_nft(
    State(_state): State<AppState>,
    _auth: AuthUser,
    Path(_id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<String>>> {
    Err(AppError::Internal("NFT minting not yet available".into()))
}
