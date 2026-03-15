//! Feed handlers — global feed, following feed.
use axum::{extract::{Query, State}, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, models::{FeedPost, FeedPostAuthor, PagedResponse}};
use crate::api::middleware::AuthUser;

#[derive(Debug, Deserialize)]
pub struct FeedQuery {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub adult: Option<bool>,
}

/// GET /api/v1/feed — global feed, newest first.
/// Works unauthenticated; pass JWT to get personalised is_liked flags.
pub async fn get_feed(
    State(state): State<AppState>,
    Query(q): Query<FeedQuery>,
) -> AppResult<Json<PagedResponse<FeedPost>>> {
    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(20).min(50).max(1);
    let show_adult = q.adult.unwrap_or(false);
    let offset = (page - 1) * per_page;

    let rows = sqlx::query!(
        r#"SELECT
            p.id, p.content, p.media_url, p.is_adult,
            p.nft_token_id, p.like_count, p.reshare_count, p.comment_count,
            p.expires_at, p.created_at,
            u.id as author_id, u.wallet_address, u.display_name, u.avatar_url
           FROM posts p
           JOIN users u ON p.author_id = u.id
           WHERE p.expires_at > NOW()
             AND ($1 OR p.is_adult = false)
           ORDER BY p.created_at DESC
           LIMIT $2 OFFSET $3"#,
        show_adult, per_page, offset
    )
    .fetch_all(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    let total: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM posts WHERE expires_at > NOW() AND ($1 OR is_adult = false)",
        show_adult
    )
    .fetch_one(state.db.pool())
    .await
    .map_err(AppError::Database)?
    .unwrap_or(0);

    let posts: Vec<FeedPost> = rows.into_iter().map(|r| FeedPost {
        id: r.id,
        content: r.content,
        media_url: r.media_url,
        is_adult: r.is_adult,
        is_nft: r.nft_token_id.is_some(),
        like_count: r.like_count,
        reshare_count: r.reshare_count,
        comment_count: r.comment_count,
        is_liked: false,
        expires_at: r.expires_at,
        created_at: r.created_at,
        author: FeedPostAuthor {
            id: r.author_id,
            wallet_address: r.wallet_address,
            display_name: r.display_name,
            avatar_url: r.avatar_url,
        },
    }).collect();

    Ok(Json(PagedResponse { success: true, data: posts, total, page, per_page }))
}

/// GET /api/v1/feed/following — posts from followed users.
pub async fn get_following_feed(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<FeedQuery>,
) -> AppResult<Json<PagedResponse<FeedPost>>> {
    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(20).min(50).max(1);
    let show_adult = q.adult.unwrap_or(false);
    let offset = (page - 1) * per_page;

    let user = sqlx::query!("SELECT id FROM users WHERE wallet_address = $1", auth.address)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))?;

    let rows = sqlx::query!(
        r#"SELECT
            p.id, p.content, p.media_url, p.is_adult,
            p.nft_token_id, p.like_count, p.reshare_count, p.comment_count,
            p.expires_at, p.created_at,
            u.id as author_id, u.wallet_address, u.display_name, u.avatar_url
           FROM posts p
           JOIN users u ON p.author_id = u.id
           JOIN follows f ON f.following_id = p.author_id
           WHERE f.follower_id = $1
             AND p.expires_at > NOW()
             AND ($2 OR p.is_adult = false)
           ORDER BY p.created_at DESC
           LIMIT $3 OFFSET $4"#,
        user.id, show_adult, per_page, offset
    )
    .fetch_all(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    let posts: Vec<FeedPost> = rows.into_iter().map(|r| FeedPost {
        id: r.id,
        content: r.content,
        media_url: r.media_url,
        is_adult: r.is_adult,
        is_nft: r.nft_token_id.is_some(),
        like_count: r.like_count,
        reshare_count: r.reshare_count,
        comment_count: r.comment_count,
        is_liked: false,
        expires_at: r.expires_at,
        created_at: r.created_at,
        author: FeedPostAuthor {
            id: r.author_id,
            wallet_address: r.wallet_address,
            display_name: r.display_name,
            avatar_url: r.avatar_url,
        },
    }).collect();

    Ok(Json(PagedResponse { success: true, data: posts, total: posts.len() as i64, page, per_page }))
}
