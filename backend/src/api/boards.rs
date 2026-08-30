//! Webboard / RSS board handlers.
//!
//! `get_boards`/`get_board` expose a static list of suggested feeds. The
//! `mine`/`add`/`remove` handlers let a logged-in user compose their OWN
//! webboard: a personal, saved list of RSS/Atom feeds (`webboard_connections`),
//! scoped to their user id. Feeds are browsed client-side; nothing here fetches
//! external URLs server-side.
use axum::{extract::{Path, State}, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;

#[derive(Debug, Serialize)]
pub struct Board {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub rss_url: &'static str,
    pub icon: &'static str,
    pub category: &'static str,
}

const BOARDS: &[Board] = &[
    Board {
        id: "cointelegraph",
        name: "CoinTelegraph",
        description: "Crypto news & analysis",
        rss_url: "https://cointelegraph.com/rss",
        icon: "CT",
        category: "news",
    },
    Board {
        id: "decrypt",
        name: "Decrypt",
        description: "Web3 news & analysis",
        rss_url: "https://decrypt.co/feed",
        icon: "DC",
        category: "news",
    },
    Board {
        id: "thedefiant",
        name: "The Defiant",
        description: "DeFi news",
        rss_url: "https://thedefiant.io/feed",
        icon: "DF",
        category: "defi",
    },
    Board {
        id: "nftnow",
        name: "NFT Now",
        description: "NFT news & drops",
        rss_url: "https://nftnow.com/feed/",
        icon: "NN",
        category: "nft",
    },
];

pub async fn get_boards(
    State(_state): State<AppState>,
) -> AppResult<Json<ApiResponse<Vec<serde_json::Value>>>> {
    let boards: Vec<serde_json::Value> = BOARDS.iter().map(|b| serde_json::json!({
        "id": b.id,
        "name": b.name,
        "description": b.description,
        "rss_url": b.rss_url,
        "icon": b.icon,
        "category": b.category,
    })).collect();

    Ok(Json(ApiResponse::ok(boards)))
}

pub async fn get_board(
    State(_state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> AppResult<Json<ApiResponse<serde_json::Value>>> {
    let board = BOARDS.iter().find(|b| b.id == id.as_str())
        .ok_or_else(|| crate::AppError::NotFound("Board not found".into()))?;

    Ok(Json(ApiResponse::ok(serde_json::json!({
        "id": board.id,
        "name": board.name,
        "description": board.description,
        "rss_url": board.rss_url,
        "icon": board.icon,
        "category": board.category,
    }))))
}

// ───────────────────────── user-composed webboards ─────────────────────────

/// Resolve the caller's user id from the JWT subject (wallet or `email:<uuid>`).
async fn caller_user_id(state: &AppState, auth: &AuthUser) -> AppResult<Uuid> {
    if let Some(rest) = auth.address.strip_prefix("email:") {
        return Uuid::parse_str(rest).map_err(|_| AppError::Validation("Invalid user id".into()));
    }
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE wallet_address = $1")
        .bind(&auth.address)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct UserFeed {
    pub id: Uuid,
    pub title: String,
    pub domain: String,
    pub feed_url: String,
    pub is_active: bool,
}

/// GET /api/v1/webboards/mine — the caller's saved feeds.
pub async fn list_mine(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<Vec<UserFeed>>>> {
    let user_id = caller_user_id(&state, &auth).await?;
    let rows = sqlx::query_as::<_, UserFeed>(
        "SELECT id, COALESCE(NULLIF(title, ''), domain) AS title, domain, feed_url, is_active
           FROM webboard_connections WHERE user_id = $1 ORDER BY created_at DESC"
    )
    .bind(user_id)
    .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(rows)))
}

#[derive(Debug, Deserialize)]
pub struct AddFeedRequest {
    pub feed_url: String,
    pub title: Option<String>,
}

/// POST /api/v1/webboards — add a feed to the caller's board.
pub async fn add_feed(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<AddFeedRequest>,
) -> AppResult<Json<ApiResponse<UserFeed>>> {
    let user_id = caller_user_id(&state, &auth).await?;

    let url = req.feed_url.trim();
    // Validate: must be a real http(s) URL with a host (the host is the
    // per-user uniqueness key + fallback label). No server-side fetch.
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| AppError::Validation("Invalid feed URL".into()))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AppError::Validation("Feed URL must start with http:// or https://".into()));
    }
    let domain = parsed.host_str()
        .ok_or_else(|| AppError::Validation("Feed URL has no host".into()))?
        .to_lowercase();
    let title = req.title.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(&domain);

    let row = sqlx::query_as::<_, UserFeed>(
        "INSERT INTO webboard_connections (user_id, domain, feed_url, title)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (user_id, domain) DO NOTHING
         RETURNING id, COALESCE(NULLIF(title, ''), domain) AS title, domain, feed_url, is_active"
    )
    .bind(user_id).bind(&domain).bind(url).bind(title)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;

    row.map(|r| Json(ApiResponse::ok(r)))
        .ok_or_else(|| AppError::Validation("You already added a feed from this site".into()))
}

/// DELETE /api/v1/webboards/:id — remove one of the caller's feeds.
pub async fn remove_feed(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<()>>> {
    let user_id = caller_user_id(&state, &auth).await?;
    let res = sqlx::query("DELETE FROM webboard_connections WHERE id = $1 AND user_id = $2")
        .bind(id).bind(user_id)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Feed not found".into()));
    }
    Ok(Json(ApiResponse::ok(())))
}
