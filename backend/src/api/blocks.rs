//! Block / unblock other users.
//!
//! Blocks are symmetric in effect: when A blocks B we (a) remove follows
//! in both directions, (b) hide the existing 1:1 DM conversation for
//! both, (c) prevent new messages/tips from either side, (d) filter B
//! out of A's feeds and A out of B's feeds (see `feed.rs`).
//!
//! Unblock removes the row but does *not* restore follows — the user
//! has to re-follow explicitly.

use axum::{extract::{Path, State}, Json};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;

/// Resolve the caller's user id from the JWT subject.
/// Mirrors the pattern used in paper_wallets / tips.
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

/// Resolve a target user (UUID, 0x-wallet, or @username).
async fn resolve_user(state: &AppState, address_or_id: &str) -> AppResult<Uuid> {
    crate::api::conversations::resolve_user(state.db.pool(), address_or_id).await
}

#[derive(Debug, Serialize)]
pub struct BlockedUser {
    pub id: Uuid,
    pub wallet_address: Option<String>,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub async fn block(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(address): Path<String>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    let blocker = caller_user_id(&state, &auth).await?;
    let blocked = resolve_user(&state, &address).await?;
    if blocker == blocked {
        return Err(AppError::Validation("You can't block yourself".into()));
    }

    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;

    // 1. Record the block (idempotent).
    sqlx::query(
        "INSERT INTO user_blocks (blocker_id, blocked_id)
         VALUES ($1, $2) ON CONFLICT DO NOTHING"
    )
    .bind(blocker).bind(blocked)
    .execute(&mut *tx).await.map_err(AppError::Database)?;

    // 2. Clear reciprocal follows in *both* directions.
    sqlx::query(
        "DELETE FROM follows
          WHERE (follower_id = $1 AND following_id = $2)
             OR (follower_id = $2 AND following_id = $1)"
    )
    .bind(blocker).bind(blocked)
    .execute(&mut *tx).await.map_err(AppError::Database)?;

    // 3. Hide the existing 1:1 DM conversation (if any) for both parties.
    let dm_pair_key = if blocker < blocked {
        format!("{}:{}", blocker, blocked)
    } else {
        format!("{}:{}", blocked, blocker)
    };
    sqlx::query(
        "UPDATE conversation_members
            SET hidden_at = COALESCE(hidden_at, NOW())
          WHERE conversation_id IN (
                SELECT id FROM conversations
                 WHERE kind = 'dm' AND dm_pair_key = $1)
            AND user_id IN ($2, $3)"
    )
    .bind(&dm_pair_key).bind(blocker).bind(blocked)
    .execute(&mut *tx).await.map_err(AppError::Database)?;

    // 4. Remove both parties from any group conversations they share so
    //    a block stops both DMs *and* group-chat reach. The remaining
    //    members' group_key envelopes are NULL'd to force a key rotation
    //    on the next admin load — same protocol as `kick`.
    let shared_groups: Vec<Uuid> = sqlx::query_scalar(
        "SELECT cm1.conversation_id
           FROM conversation_members cm1
           JOIN conversation_members cm2
             ON cm1.conversation_id = cm2.conversation_id
           JOIN conversations c ON c.id = cm1.conversation_id
          WHERE c.kind = 'group'
            AND cm1.user_id = $1
            AND cm2.user_id = $2"
    )
    .bind(blocker).bind(blocked)
    .fetch_all(&mut *tx).await.map_err(AppError::Database)?;

    if !shared_groups.is_empty() {
        sqlx::query(
            "DELETE FROM conversation_members
              WHERE conversation_id = ANY($1) AND user_id IN ($2, $3)"
        )
        .bind(&shared_groups[..]).bind(blocker).bind(blocked)
        .execute(&mut *tx).await.map_err(AppError::Database)?;

        sqlx::query(
            "UPDATE conversation_members
                SET encrypted_group_key = NULL
              WHERE conversation_id = ANY($1)"
        )
        .bind(&shared_groups[..])
        .execute(&mut *tx).await.map_err(AppError::Database)?;
    }

    tx.commit().await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok("blocked")))
}

pub async fn unblock(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(address): Path<String>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    let blocker = caller_user_id(&state, &auth).await?;
    let blocked = resolve_user(&state, &address).await?;

    sqlx::query("DELETE FROM user_blocks WHERE blocker_id = $1 AND blocked_id = $2")
        .bind(blocker).bind(blocked)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok("unblocked")))
}

pub async fn list_mine(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<Vec<BlockedUser>>>> {
    let me = caller_user_id(&state, &auth).await?;

    let rows: Vec<(Uuid, Option<String>, Option<String>, Option<String>, Option<String>, DateTime<Utc>)> =
        sqlx::query_as(
            "SELECT u.id, u.wallet_address, u.username, u.display_name, u.avatar_url, ub.created_at
               FROM user_blocks ub
               JOIN users u ON u.id = ub.blocked_id
              WHERE ub.blocker_id = $1
              ORDER BY ub.created_at DESC"
        )
        .bind(me)
        .fetch_all(state.db.pool())
        .await
        .map_err(AppError::Database)?;

    let out = rows.into_iter().map(|r| BlockedUser {
        id: r.0, wallet_address: r.1, username: r.2,
        display_name: r.3, avatar_url: r.4, created_at: r.5,
    }).collect();

    Ok(Json(ApiResponse::ok(out)))
}

/// Returns true if either party has blocked the other. Used to gate
/// new messages, tips, follows, etc. Centralised here so the rule stays
/// consistent across the codebase.
pub async fn either_blocks(
    pool: &sqlx::PgPool,
    user_a: Uuid,
    user_b: Uuid,
) -> AppResult<bool> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_blocks
          WHERE (blocker_id = $1 AND blocked_id = $2)
             OR (blocker_id = $2 AND blocked_id = $1)"
    )
    .bind(user_a).bind(user_b)
    .fetch_one(pool).await.map_err(AppError::Database)?;
    Ok(n > 0)
}
