use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::AppState;
use shared::{ApiResponse, Post, PostVisibility, PostSource, Comment};

#[derive(Deserialize)]
pub struct CreatePostRequest {
    pub content: String,
    pub media_urls: Vec<String>,
    pub visibility: PostVisibility,
    pub pay_per_view_price: Option<f64>,
}

pub async fn create_post(
    State(state): State<AppState>,
    Json(req): Json<CreatePostRequest>,
) -> Result<Json<ApiResponse<Post>>, StatusCode> {
    // TODO: extract user_id from JWT middleware
    let author_id = Uuid::new_v4(); // placeholder

    // Posts expire in 24h (hybrid logic: DB-side timer)
    let expires_at = Utc::now() + Duration::hours(24);

    let row = sqlx::query!(
        r#"
        INSERT INTO posts (
            id, author_id, content, media_urls, visibility,
            pay_per_view_price, expires_at, created_at
        )
        VALUES ($1, $2, $3, $4, $5::post_visibility, $6, $7, NOW())
        RETURNING id, created_at
        "#,
        Uuid::new_v4(),
        author_id,
        req.content,
        &req.media_urls,
        serde_json::to_string(&req.visibility).unwrap().trim_matches('"'),
        req.pay_per_view_price,
        expires_at,
    )
    .fetch_one(&state.db.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Reward token for posting
    let _ = crate::services::tokens::reward_action(
        &state, author_id, shared::RewardAction::Share
    ).await;

    let post = Post {
        id: row.id,
        author_id,
        author_username: "unknown".into(),
        content: req.content,
        media_urls: req.media_urls,
        visibility: req.visibility,
        source: PostSource::Yeet,
        pay_per_view_price: req.pay_per_view_price,
        is_nft: false,
        nft_token_id: None,
        nft_contract: None,
        like_count: 0,
        comment_count: 0,
        reshare_count: 0,
        tip_total: 0.0,
        expires_at,
        created_at: row.created_at,
        reshared_from: None,
    };

    Ok(Json(ApiResponse::ok(post)))
}

pub async fn get_post(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<Post>>, StatusCode> {
    let row = sqlx::query!(
        r#"
        SELECT p.*, u.username as author_username
        FROM posts p
        JOIN users u ON u.id = p.author_id
        WHERE p.id = $1
          AND p.expires_at > NOW()
          AND p.deleted_at IS NULL
        "#,
        id
    )
    .fetch_optional(&state.db.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match row {
        Some(r) => {
            let post = Post {
                id: r.id,
                author_id: r.author_id,
                author_username: r.author_username,
                content: r.content,
                media_urls: r.media_urls.unwrap_or_default(),
                visibility: serde_json::from_str(
                    &format!("\"{}\"", r.visibility)
                ).unwrap_or(PostVisibility::Public),
                source: PostSource::Yeet,
                pay_per_view_price: r.pay_per_view_price.map(|v| v.to_f64().unwrap_or(0.0)),
                is_nft: r.is_nft,
                nft_token_id: r.nft_token_id,
                nft_contract: r.nft_contract_address,
                like_count: r.like_count.unwrap_or(0),
                comment_count: r.comment_count.unwrap_or(0),
                reshare_count: r.reshare_count.unwrap_or(0),
                tip_total: r.tip_total.map(|v| v.to_f64().unwrap_or(0.0)).unwrap_or(0.0),
                expires_at: r.expires_at,
                created_at: r.created_at,
                reshared_from: r.reshared_from,
            };
            Ok(Json(ApiResponse::ok(post)))
        }
        None => Ok(Json(ApiResponse::err("Post not found or expired"))),
    }
}

pub async fn delete_post(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    // Soft-delete (NFT posts can't be truly deleted — only hidden from feed)
    sqlx::query!(
        "UPDATE posts SET deleted_at = NOW() WHERE id = $1 AND is_nft = false",
        id
    )
    .execute(&state.db.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(ApiResponse::ok(())))
}

pub async fn like_post(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<i64>>, StatusCode> {
    let row = sqlx::query!(
        r#"
        INSERT INTO post_likes (post_id, user_id, created_at)
        VALUES ($1, $2, NOW())
        ON CONFLICT DO NOTHING;
        SELECT COUNT(*) as "count!" FROM post_likes WHERE post_id = $1
        "#,
        id,
        Uuid::new_v4(), // TODO: from JWT
    )
    .fetch_one(&state.db.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(ApiResponse::ok(row.count)))
}

pub async fn reshare_post(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<Post>>, StatusCode> {
    // Reset the 24h timer on the original post (Hybrid logic!)
    sqlx::query!(
        r#"
        UPDATE posts
        SET expires_at = GREATEST(expires_at, NOW() + INTERVAL '24 hours'),
            reshare_count = reshare_count + 1
        WHERE id = $1 AND is_nft = false
        "#,
        id
    )
    .execute(&state.db.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Reward resharer
    let _ = crate::services::tokens::reward_action(
        &state, Uuid::new_v4(), shared::RewardAction::Reshare
    ).await;

    // Return fresh post
    get_post(State(state), Path(id)).await.map(|r| r)
}

#[derive(Deserialize)]
pub struct CreateCommentRequest {
    pub content: String,
}

pub async fn get_comments(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<Vec<Comment>>>, StatusCode> {
    let rows = sqlx::query!(
        r#"
        SELECT c.id, c.post_id, c.author_id, u.username, c.content, c.created_at
        FROM comments c
        JOIN users u ON u.id = c.author_id
        WHERE c.post_id = $1
        ORDER BY c.created_at ASC
        LIMIT 100
        "#,
        id
    )
    .fetch_all(&state.db.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let comments = rows.into_iter().map(|r| Comment {
        id: r.id,
        post_id: r.post_id,
        author_id: r.author_id,
        author_username: r.username,
        content: r.content,
        created_at: r.created_at,
    }).collect();

    Ok(Json(ApiResponse::ok(comments)))
}

pub async fn add_comment(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateCommentRequest>,
) -> Result<Json<ApiResponse<Comment>>, StatusCode> {
    let author_id = Uuid::new_v4(); // TODO: from JWT

    let row = sqlx::query!(
        r#"
        INSERT INTO comments (id, post_id, author_id, content, created_at)
        VALUES ($1, $2, $3, $4, NOW())
        RETURNING id, created_at
        "#,
        Uuid::new_v4(), id, author_id, req.content
    )
    .fetch_one(&state.db.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let _ = crate::services::tokens::reward_action(
        &state, author_id, shared::RewardAction::Comment
    ).await;

    Ok(Json(ApiResponse::ok(Comment {
        id: row.id,
        post_id: id,
        author_id,
        author_username: "unknown".into(),
        content: req.content,
        created_at: row.created_at,
    })))
}

pub async fn mint_nft(
    State(_state): State<AppState>,
    Path(_id): Path<Uuid>,
) -> Json<ApiResponse<String>> {
    // NFT minting: once minted, post gets is_nft=true and expires_at is cleared
    // Full implementation in services/nft.rs
    Json(ApiResponse::err("NFT minting: connect wallet in frontend"))
}
