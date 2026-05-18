//! Cross-entity search: users + posts.
//!
//! Single endpoint that the dropdown in the top nav calls. ILIKE-based
//! for now (good enough for the data volume; switch to pg_trgm GIN
//! indexes if the table grows). Block-aware and age-gate-aware so the
//! caller never sees content they wouldn't see in the feed.

use axum::{extract::{Query, State}, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    /// "all" (default) | "users" | "posts"
    pub kind: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UserHit {
    pub id: Uuid,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub wallet_address: Option<String>,
    pub avatar_url: Option<String>,
    pub e2ee_ready: bool,
}

#[derive(Debug, Serialize)]
pub struct PostHit {
    pub id: Uuid,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub author_id: Uuid,
    pub author_username: Option<String>,
    pub author_display_name: Option<String>,
    pub author_avatar_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub users: Vec<UserHit>,
    pub posts: Vec<PostHit>,
}

async fn caller_user_id(state: &AppState, auth: &AuthUser) -> AppResult<Uuid> {
    if let Some(rest) = auth.address.strip_prefix("email:") {
        return Uuid::parse_str(rest)
            .map_err(|_| AppError::Validation("Invalid user id".into()));
    }
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE wallet_address = $1")
        .bind(&auth.address)
        .fetch_optional(state.db.pool())
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))
}

pub async fn search(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<SearchQuery>,
) -> AppResult<Json<ApiResponse<SearchResponse>>> {
    let term = q.q.trim();
    if term.is_empty() {
        return Ok(Json(ApiResponse::ok(SearchResponse { users: vec![], posts: vec![] })));
    }
    if term.len() > 80 {
        return Err(AppError::Validation("Query too long".into()));
    }
    let kind = q.kind.as_deref().unwrap_or("all");
    let want_users = kind == "all" || kind == "users";
    let want_posts = kind == "all" || kind == "posts";

    let viewer = caller_user_id(&state, &auth).await?;

    // ILIKE pattern with the user's term wrapped in %. Strip a leading
    // @ or # so 'searching for @alice' or '#yeet' Just Works.
    let clean = term.trim_start_matches(['@', '#']);
    let pat = format!("%{}%", clean);

    let users: Vec<UserHit> = if want_users {
        let rows: Vec<(Uuid, Option<String>, Option<String>, Option<String>, Option<String>, bool)> =
            sqlx::query_as(
                "SELECT u.id, u.username, u.display_name, u.wallet_address, u.avatar_url,
                        (u.e2ee_public_key IS NOT NULL) AS e2ee_ready
                   FROM users u
                  WHERE (u.username ILIKE $1 OR u.display_name ILIKE $1
                         OR LOWER(u.wallet_address) = LOWER($2))
                    AND u.id <> $3
                    AND NOT EXISTS (SELECT 1 FROM user_blocks ub
                                     WHERE (ub.blocker_id = $3 AND ub.blocked_id = u.id)
                                        OR (ub.blocker_id = u.id AND ub.blocked_id = $3))
                  ORDER BY
                    CASE WHEN LOWER(u.username) = LOWER($4) THEN 0 ELSE 1 END,
                    CASE WHEN u.username ILIKE $5 THEN 0 ELSE 1 END,
                    u.username NULLS LAST
                  LIMIT 8"
            )
            .bind(&pat).bind(clean).bind(viewer).bind(clean).bind(format!("{}%", clean))
            .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;
        rows.into_iter().map(|r| UserHit {
            id: r.0, username: r.1, display_name: r.2,
            wallet_address: r.3, avatar_url: r.4, e2ee_ready: r.5,
        }).collect()
    } else { vec![] };

    let posts: Vec<PostHit> = if want_posts {
        // Age-gate: anonymous-style filter on is_adult unless the
        // viewer is age-verified. We treat the viewer as verified when
        // their `age_verified_at` is set.
        let verified: Option<bool> = sqlx::query_scalar(
            "SELECT age_verified_at IS NOT NULL FROM users WHERE id = $1"
        )
        .bind(viewer)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
        let allow_adult = verified.unwrap_or(false);
        let adult_clause = if allow_adult { "" } else { " AND p.is_adult = FALSE" };

        let sql = format!(
            "SELECT p.id, p.content, p.created_at,
                    u.id AS author_id, u.username, u.display_name, u.avatar_url
               FROM posts p
               JOIN users u ON u.id = p.author_id
              WHERE p.content ILIKE $1
                AND p.expires_at > NOW()
                AND p.is_removed = FALSE AND p.deleted_at IS NULL
                AND NOT EXISTS (SELECT 1 FROM user_blocks ub
                                 WHERE (ub.blocker_id = $2 AND ub.blocked_id = u.id)
                                    OR (ub.blocker_id = u.id AND ub.blocked_id = $2))
                {}
              ORDER BY p.created_at DESC
              LIMIT 12",
            adult_clause
        );
        let rows: Vec<(Uuid, String, DateTime<Utc>, Uuid, Option<String>, Option<String>, Option<String>)> =
            sqlx::query_as(&sql)
                .bind(&pat).bind(viewer)
                .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;
        rows.into_iter().map(|r| PostHit {
            id: r.0,
            content: if r.1.chars().count() > 180 {
                r.1.chars().take(180).collect::<String>() + "…"
            } else { r.1 },
            created_at: r.2,
            author_id: r.3,
            author_username: r.4,
            author_display_name: r.5,
            author_avatar_url: r.6,
        }).collect()
    } else { vec![] };

    Ok(Json(ApiResponse::ok(SearchResponse { users, posts })))
}
