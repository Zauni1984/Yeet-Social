//! Feed handlers — global + following feed.
use axum::{extract::{Query, State}, Json};
use serde::Deserialize;
use chrono::{DateTime, Utc};
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, models::{FeedPost, FeedPostAuthor, PagedResponse}};
use crate::api::middleware::AuthUser;

#[derive(Debug, Deserialize)]
pub struct FeedQuery {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub adult: Option<bool>,
}

#[derive(sqlx::FromRow)]
struct FeedRow {
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
    wallet_address: Option<String>,
    display_name: Option<String>,
    avatar_url: Option<String>,
    tip_total_yeet: Option<f64>,
}

pub async fn get_feed(
    State(state): State<AppState>,
    Query(q): Query<FeedQuery>,
) -> AppResult<Json<PagedResponse<FeedPost>>> {
    let page     = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(20).clamp(1, 50);
    let offset   = (page - 1) * per_page;

    let rows = sqlx::query_as::<_, FeedRow>(
        "SELECT p.id, p.content, p.media_urls, p.is_nft, p.nft_token_id,
                p.like_count, p.reshare_count, p.comment_count,
                p.expires_at, p.created_at,
                u.id as author_id, u.wallet_address, u.display_name, u.avatar_url
         FROM posts p JOIN users u ON p.author_id = u.id
         WHERE p.expires_at > NOW() AND p.deleted_at IS NULL
         ORDER BY p.created_at DESC LIMIT $1 OFFSET $2"
    )
    .bind(per_page).bind(offset)
    .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;

    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM posts WHERE expires_at > NOW() AND deleted_at IS NULL"
    )
    .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;

    let posts = rows.into_iter().map(row_to_feed_post).collect();
    Ok(Json(PagedResponse { success: true, data: posts, total, page, per_page }))
}

pub async fn get_following_feed(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<FeedQuery>,
) -> AppResult<Json<PagedResponse<FeedPost>>> {
    let page     = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(20).clamp(1, 50);
    let offset   = (page - 1) * per_page;

    let user_id: Uuid = if let Some(uuid_str) = auth.address.strip_prefix("email:") {
        uuid_str.parse::<Uuid>().map_err(|_| AppError::NotFound("Invalid user ID".into()))?
    } else {
        sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
            .bind(&auth.address)
            .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
            .ok_or_else(|| AppError::NotFound("User not found".into()))?
    };

    let rows = sqlx::query_as::<_, FeedRow>(
        "SELECT p.id, p.content, p.media_urls, p.is_nft, p.nft_token_id,
                p.like_count, p.reshare_count, p.comment_count,
                p.expires_at, p.created_at,
                u.id as author_id, u.wallet_address, u.display_name, u.avatar_url
         FROM posts p
         JOIN users u ON p.author_id = u.id
         JOIN follows f ON f.following_id = p.author_id
         WHERE f.follower_id = $1 AND p.expires_at > NOW() AND p.deleted_at IS NULL
         ORDER BY p.created_at DESC LIMIT $2 OFFSET $3"
    )
    .bind(user_id).bind(per_page).bind(offset)
    .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;

    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM posts p JOIN follows f ON f.following_id = p.author_id
         WHERE f.follower_id = $1 AND p.expires_at > NOW() AND p.deleted_at IS NULL"
    )
    .bind(user_id)
    .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;

    let posts = rows.into_iter().map(row_to_feed_post).collect();
    Ok(Json(PagedResponse { success: true, data: posts, total, page, per_page }))
}

fn row_to_feed_post(r: FeedRow) -> FeedPost {
    let media_url = r.media_urls.and_then(|v| v.into_iter().next());
    FeedPost {
        id: r.id,
        content: r.content,
        media_url,
        is_adult: false,
        is_nft: r.is_nft,
        like_count: r.like_count as i32,
        reshare_count: r.reshare_count as i32,
        comment_count: r.comment_count as i32,
        is_liked: false,
        expires_at: r.expires_at,
        created_at: r.created_at,
        author: FeedPostAuthor {
            id: r.author_id,
            wallet_address: r.wallet_address,
            display_name: r.display_name,
            avatar_url: r.avatar_url,
        },
        tip_total_yeet: r.tip_total_yeet,
    }
}
