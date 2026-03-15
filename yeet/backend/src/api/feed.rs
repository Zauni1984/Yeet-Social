use axum::{extract::{Query, State}, Json};
use serde::Deserialize;
use crate::AppState;
use shared::{ApiResponse, Post, PostVisibility, PostSource, FeedMode};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct FeedQuery {
    pub mode: Option<String>,    // "global" | "following" | "subscriptions"
    pub show_18_plus: Option<bool>,
    pub cursor: Option<String>,  // ISO timestamp for pagination
    pub limit: Option<i64>,
}

pub async fn get_feed(
    State(state): State<AppState>,
    Query(q): Query<FeedQuery>,
) -> Json<ApiResponse<Vec<Post>>> {
    let limit = q.limit.unwrap_or(20).min(50);
    let show_18_plus = q.show_18_plus.unwrap_or(false);
    let mode = q.mode.as_deref().unwrap_or("global");

    let rows = match mode {
        "following" => {
            // Posts from users the current user follows
            sqlx::query!(
                r#"
                SELECT p.*, u.username as author_username
                FROM posts p
                JOIN users u ON u.id = p.author_id
                JOIN follows f ON f.following_id = p.author_id
                WHERE f.follower_id = $1
                  AND p.expires_at > NOW()
                  AND p.deleted_at IS NULL
                  AND ($2 OR p.visibility != 'age_restricted')
                ORDER BY p.created_at DESC
                LIMIT $3
                "#,
                Uuid::nil(), // TODO: from JWT
                show_18_plus,
                limit,
            )
            .fetch_all(&state.db.pool)
            .await
        }
        _ => {
            // Global timeline — all public + web board posts
            sqlx::query!(
                r#"
                SELECT p.*, u.username as author_username
                FROM posts p
                JOIN users u ON u.id = p.author_id
                WHERE p.expires_at > NOW()
                  AND p.deleted_at IS NULL
                  AND p.visibility = 'public'
                  AND ($1 OR p.visibility != 'age_restricted')
                ORDER BY p.created_at DESC
                LIMIT $2
                "#,
                show_18_plus,
                limit,
            )
            .fetch_all(&state.db.pool)
            .await
        }
    };

    match rows {
        Ok(rows) => {
            let posts = rows.into_iter().map(|r| Post {
                id: r.id,
                author_id: r.author_id,
                author_username: r.author_username,
                content: r.content,
                media_urls: r.media_urls.unwrap_or_default(),
                visibility: serde_json::from_str(
                    &format!("\"{}\"", r.visibility)
                ).unwrap_or(PostVisibility::Public),
                source: PostSource::Yeet,
                pay_per_view_price: r.pay_per_view_price
                    .map(|v| v.to_f64().unwrap_or(0.0)),
                is_nft: r.is_nft,
                nft_token_id: r.nft_token_id,
                nft_contract: r.nft_contract_address,
                like_count: r.like_count.unwrap_or(0),
                comment_count: r.comment_count.unwrap_or(0),
                reshare_count: r.reshare_count.unwrap_or(0),
                tip_total: r.tip_total
                    .map(|v| v.to_f64().unwrap_or(0.0)).unwrap_or(0.0),
                expires_at: r.expires_at,
                created_at: r.created_at,
                reshared_from: r.reshared_from,
            }).collect();
            Json(ApiResponse::ok(posts))
        }
        Err(_) => Json(ApiResponse::err("Feed query failed")),
    }
}
