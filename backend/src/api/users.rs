//! User profile, follow/unfollow handlers.
use axum::{extract::{Path, State}, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, models::{ApiResponse, UserProfile}};
use crate::api::middleware::{AuthUser, OptionalAuth};

#[derive(sqlx::FromRow)]
struct ProfileRow {
    id: Uuid, wallet_address: Option<String>, display_name: Option<String>,
    bio: Option<String>, avatar_url: Option<String>, cover_url: Option<String>, created_at: DateTime<Utc>,
    follower_count: Option<i64>, following_count: Option<i64>, post_count: Option<i64>,
    age_verified_at: Option<DateTime<Utc>>,
    age_badge_hidden: Option<bool>,
    e2ee_public_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProfileRequest {
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub avatar_url: Option<String>,
    pub adult_content: Option<bool>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct FollowEntry {
    pub id: Uuid,
    pub wallet_address: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub followed_at: DateTime<Utc>,
    #[serde(default)]
    pub username: Option<String>,
    // True iff this user has uploaded an E2EE public key — i.e. is
    // reachable for encrypted DMs / can be added to encrypted groups.
    #[serde(default)]
    pub e2ee_ready: bool,
}

pub async fn get_profile(
    State(state): State<AppState>,
    OptionalAuth(viewer): OptionalAuth,
    Path(address): Path<String>,
) -> AppResult<Json<ApiResponse<UserProfile>>> {
    // Accept either a UUID (email-only users) or a wallet address
    let query = if address.parse::<Uuid>().is_ok() {
        "SELECT u.id, u.wallet_address, u.display_name, u.bio, u.avatar_url, u.cover_url, u.created_at,
                (SELECT COUNT(*) FROM follows WHERE following_id = u.id)::bigint as follower_count,
                (SELECT COUNT(*) FROM follows WHERE follower_id  = u.id)::bigint as following_count,
                (SELECT COUNT(*) FROM posts WHERE author_id = u.id)::bigint as post_count,
                u.age_verified_at, u.age_badge_hidden, u.e2ee_public_key
         FROM users u WHERE u.id = $1::uuid"
    } else {
        "SELECT u.id, u.wallet_address, u.display_name, u.bio, u.avatar_url, u.cover_url, u.created_at,
                (SELECT COUNT(*) FROM follows WHERE following_id = u.id)::bigint as follower_count,
                (SELECT COUNT(*) FROM follows WHERE follower_id  = u.id)::bigint as following_count,
                (SELECT COUNT(*) FROM posts WHERE author_id = u.id)::bigint as post_count,
                u.age_verified_at, u.age_badge_hidden, u.e2ee_public_key
         FROM users u WHERE u.wallet_address = $1"
    };
    let bind_val = if address.parse::<Uuid>().is_ok() { address.clone() } else { address.to_lowercase() };
    let r = sqlx::query_as::<_, ProfileRow>(query)
        .bind(bind_val)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))?;

    // Block + follow state are only meaningful when the caller is signed in.
    let (is_blocked_by_me, has_blocked_me, is_following) = if let Some(auth) = viewer.as_ref() {
        if let Ok(viewer_id) = resolve_user_id(&state, &auth.address).await {
            if viewer_id == r.id {
                (false, false, false)
            } else {
                let blocked: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM user_blocks WHERE blocker_id = $1 AND blocked_id = $2)"
                ).bind(viewer_id).bind(r.id)
                 .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
                let blocked_by: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM user_blocks WHERE blocker_id = $1 AND blocked_id = $2)"
                ).bind(r.id).bind(viewer_id)
                 .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
                let following: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM follows WHERE follower_id = $1 AND following_id = $2)"
                ).bind(viewer_id).bind(r.id)
                 .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
                (blocked, blocked_by, following)
            }
        } else { (false, false, false) }
    } else {
        (false, false, false)
    };

    Ok(Json(ApiResponse::ok(UserProfile {
        id: r.id, wallet_address: r.wallet_address.clone(), display_name: r.display_name,
        bio: r.bio, avatar_url: r.avatar_url, cover_url: r.cover_url,
        follower_count: r.follower_count.unwrap_or(0),
        following_count: r.following_count.unwrap_or(0),
        post_count: r.post_count.unwrap_or(0),
        // Public-facing semantics: the badge is shown when the user
        // is age-verified AND hasn't toggled the badge off. The user's
        // own /me/age-verification/status endpoint reveals the
        // underlying state separately.
        age_verified: r.age_verified_at.is_some() && !r.age_badge_hidden.unwrap_or(false),
        created_at: r.created_at,
        is_blocked_by_me,
        has_blocked_me,
        e2ee_ready: r.e2ee_public_key.is_some(),
        is_following,
    })))
}

pub async fn get_my_profile(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<UserProfile>>> {
    // Email users carry "email:UUID" in auth.address — look up by UUID; wallet users keep 0x...
    let key = auth.address.strip_prefix("email:").unwrap_or(&auth.address).to_string();
    get_profile(State(state), OptionalAuth(Some(auth)), Path(key)).await
}

pub async fn update_profile(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<UpdateProfileRequest>,
) -> AppResult<Json<ApiResponse<()>>> {
    if let Some(ref n) = req.display_name { if n.len() > 50 { return Err(AppError::Validation("Display name max 50 chars".into())); } }
    if let Some(ref b) = req.bio { if b.len() > 280 { return Err(AppError::Validation("Bio max 280 chars".into())); } }

    let user_id = resolve_user_id(&state, &auth.address).await?;
    sqlx::query(
        "UPDATE users SET
            display_name  = COALESCE($2, display_name),
            bio           = COALESCE($3, bio),
            avatar_url    = COALESCE($4, avatar_url),
            updated_at    = NOW()
         WHERE id = $1"
    )
    .bind(user_id).bind(&req.display_name).bind(&req.bio)
    .bind(&req.avatar_url)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;

    if let Some(nsfw) = req.adult_content {
        sqlx::query(
            "INSERT INTO user_settings (user_id, show_nsfw) VALUES ($1, $2)
             ON CONFLICT (user_id) DO UPDATE SET show_nsfw = $2, updated_at = NOW()"
        )
        .bind(user_id).bind(nsfw)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;
    }
    Ok(Json(ApiResponse::ok(())))
}

// Resolve user UUID from auth (supports both wallet and email users)
async fn resolve_user_id(state: &AppState, auth_address: &str) -> AppResult<Uuid> {
    if let Some(uuid_str) = auth_address.strip_prefix("email:") {
        return uuid_str.parse::<Uuid>().map_err(|_| AppError::NotFound("Invalid user ID".into()));
    }
    sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
        .bind(auth_address)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))
}

// Resolve target user UUID from address or user ID string
async fn resolve_target_id(state: &AppState, address: &str) -> AppResult<Uuid> {
    // Try as UUID directly (for email users referenced by ID)
    if let Ok(uuid) = address.parse::<Uuid>() {
        return Ok(uuid);
    }
    sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
        .bind(address.to_lowercase())
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("Target user not found".into()))
}

pub async fn follow_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(address): Path<String>,
) -> AppResult<Json<ApiResponse<()>>> {
    let follower_id = resolve_user_id(&state, &auth.address).await?;
    let following_id = resolve_target_id(&state, &address).await?;
    if follower_id == following_id {
        return Err(AppError::Validation("Cannot follow yourself".into()));
    }
    let inserted = sqlx::query(
        "INSERT INTO follows (follower_id, following_id) VALUES ($1, $2) ON CONFLICT DO NOTHING"
    )
    .bind(follower_id).bind(following_id)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;
    // Only notify on a brand-new follow row (not a duplicate POST).
    if inserted.rows_affected() > 0 {
        let actor_name = sqlx::query_scalar::<_, Option<String>>(
            "SELECT COALESCE(display_name, username) FROM users WHERE id = $1"
        )
        .bind(follower_id)
        .fetch_optional(state.db.pool()).await
        .ok().flatten().flatten().unwrap_or_else(|| "Someone".into());
        crate::api::notifications::notify(
            state.db.pool(), following_id, Some(follower_id),
            "follow", &format!("{} started following you", actor_name), None,
        ).await;
    }
    Ok(Json(ApiResponse::ok(())))
}

pub async fn unfollow_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(address): Path<String>,
) -> AppResult<Json<ApiResponse<()>>> {
    let follower_id = resolve_user_id(&state, &auth.address).await?;
    let following_id = resolve_target_id(&state, &address).await?;
    sqlx::query("DELETE FROM follows WHERE follower_id = $1 AND following_id = $2")
        .bind(follower_id).bind(following_id)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(())))
}

// ---- DSGVO account actions ----

pub async fn export_my_data(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<serde_json::Value>> {
    let user_id = resolve_user_id(&state, &auth.address).await?;

    let user: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT to_jsonb(u) - 'password_hash' - 'password_salt'
           FROM users u WHERE u.id = $1"
    )
    .bind(user_id).fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;

    let posts: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT to_jsonb(p) FROM posts p WHERE p.author_id = $1 ORDER BY p.created_at DESC"
    )
    .bind(user_id).fetch_all(state.db.pool()).await.map_err(AppError::Database)?;

    let settings: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT to_jsonb(s) FROM user_settings s WHERE s.user_id = $1"
    )
    .bind(user_id).fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;

    let followers: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT to_jsonb(f) FROM follows f WHERE f.following_id = $1"
    )
    .bind(user_id).fetch_all(state.db.pool()).await.map_err(AppError::Database)?;

    let following: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT to_jsonb(f) FROM follows f WHERE f.follower_id = $1"
    )
    .bind(user_id).fetch_all(state.db.pool()).await.map_err(AppError::Database)?;

    Ok(Json(serde_json::json!({
        "exported_at": Utc::now(),
        "user": user,
        "settings": settings,
        "posts": posts,
        "followers": followers,
        "following": following,
    })))
}

pub async fn verify_age(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<serde_json::Value>>> {
    let user_id = resolve_user_id(&state, &auth.address).await?;
    sqlx::query("UPDATE users SET age_verified_at = COALESCE(age_verified_at, NOW()) WHERE id = $1")
        .bind(user_id)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(serde_json::json!({"age_verified": true}))))
}

pub async fn delete_my_account(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<serde_json::Value>>> {
    let user_id = resolve_user_id(&state, &auth.address).await?;
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(serde_json::json!({"deleted": true}))))
}

pub async fn list_followers(
    State(state): State<AppState>,
    Path(address): Path<String>,
) -> AppResult<Json<ApiResponse<Vec<FollowEntry>>>> {
    let target_id = resolve_target_id(&state, &address).await?;
    let rows = sqlx::query_as::<_, FollowEntry>(
        "SELECT u.id, u.wallet_address, u.display_name, u.avatar_url, f.created_at AS followed_at,
                u.username, (u.e2ee_public_key IS NOT NULL) AS e2ee_ready
           FROM follows f JOIN users u ON u.id = f.follower_id
          WHERE f.following_id = $1
          ORDER BY f.created_at DESC
          LIMIT 200"
    )
    .bind(target_id)
    .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(rows)))
}

pub async fn list_following(
    State(state): State<AppState>,
    Path(address): Path<String>,
) -> AppResult<Json<ApiResponse<Vec<FollowEntry>>>> {
    let target_id = resolve_target_id(&state, &address).await?;
    let rows = sqlx::query_as::<_, FollowEntry>(
        "SELECT u.id, u.wallet_address, u.display_name, u.avatar_url, f.created_at AS followed_at,
                u.username, (u.e2ee_public_key IS NOT NULL) AS e2ee_ready
           FROM follows f JOIN users u ON u.id = f.following_id
          WHERE f.follower_id = $1
          ORDER BY f.created_at DESC
          LIMIT 200"
    )
    .bind(target_id)
    .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(rows)))
}
