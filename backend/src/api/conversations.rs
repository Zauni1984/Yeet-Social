//! Conversations (DM + group) — list, create, settings.
//!
//! Messages are in `api::messages`. Group invitation flow is in
//! `api::invitations`. Both modules call helpers exposed here so the
//! membership invariants stay in one place.

use axum::{extract::{Path, State}, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;

pub(crate) async fn caller_user_id(state: &AppState, auth: &AuthUser) -> AppResult<Uuid> {
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

/// Resolve a user reference (UUID, 0x-wallet, or @username) to a UUID.
pub(crate) async fn resolve_user(pool: &PgPool, address_or_id: &str) -> AppResult<Uuid> {
    let raw = address_or_id.trim().trim_start_matches('@');
    if let Ok(id) = Uuid::parse_str(raw) {
        return Ok(id);
    }
    // wallet_address lookup (case-insensitive)
    if let Some(id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM users WHERE LOWER(wallet_address) = LOWER($1)"
    )
    .bind(raw)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)?
    {
        return Ok(id);
    }
    // username lookup (case-insensitive, exact match)
    if let Some(id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM users WHERE LOWER(username) = LOWER($1)"
    )
    .bind(raw)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)?
    {
        return Ok(id);
    }
    Err(AppError::NotFound("User not found".into()))
}

pub(crate) async fn assert_member(pool: &PgPool, conv: Uuid, user: Uuid) -> AppResult<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM conversation_members WHERE conversation_id = $1 AND user_id = $2)"
    )
    .bind(conv).bind(user)
    .fetch_one(pool).await.map_err(AppError::Database)?;
    if !exists {
        return Err(AppError::Forbidden("Not a member of this conversation".into()));
    }
    Ok(())
}

fn dm_pair_key(a: Uuid, b: Uuid) -> String {
    if a < b { format!("{}:{}", a, b) } else { format!("{}:{}", b, a) }
}

// ---------- DM creation ----------

#[derive(Debug, Deserialize)]
pub struct CreateDmRequest {
    pub peer_address: String,
}

#[derive(Debug, Serialize)]
pub struct ConversationSummary {
    pub id: Uuid,
    pub kind: String,
    pub name: Option<String>,
    pub peer_id: Option<Uuid>,
    pub peer_username: Option<String>,
    pub peer_display_name: Option<String>,
    pub peer_avatar_url: Option<String>,
    pub peer_public_key: Option<String>,
    // For group conversations: this caller's envelope of the
    // group_key (NULL until the admin (re-)distributes it).
    pub encrypted_group_key: Option<String>,
    // Who wrapped the encrypted_group_key above + their current
    // e2ee_public_key. The invitee derives ECDH(self_sk, wrapper_pk)
    // to unwrap. For the creator's own envelope this is self.
    pub wrapper_user_id: Option<Uuid>,
    pub wrapper_public_key: Option<String>,
    pub role: Option<String>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

pub async fn create_dm(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateDmRequest>,
) -> AppResult<Json<ApiResponse<ConversationSummary>>> {
    let me = caller_user_id(&state, &auth).await?;
    let peer = resolve_user(state.db.pool(), &req.peer_address).await?;
    if me == peer {
        return Err(AppError::Validation("Cannot DM yourself".into()));
    }
    if crate::api::blocks::either_blocks(state.db.pool(), me, peer).await? {
        return Err(AppError::Forbidden("Blocked".into()));
    }

    let pair_key = dm_pair_key(me, peer);
    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;

    // Idempotent create: if a conversation already exists for this DM
    // pair, return it; otherwise insert it and the two members.
    let existing: Option<(Uuid, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, created_at FROM conversations WHERE kind='dm' AND dm_pair_key = $1"
    )
    .bind(&pair_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    let (conv_id, created_at) = if let Some(row) = existing {
        // Un-hide the conversation for the caller if it was soft-hidden.
        sqlx::query(
            "UPDATE conversation_members SET hidden_at = NULL
              WHERE conversation_id = $1 AND user_id = $2"
        )
        .bind(row.0).bind(me)
        .execute(&mut *tx).await.map_err(AppError::Database)?;
        row
    } else {
        let conv: (Uuid, DateTime<Utc>) = sqlx::query_as(
            "INSERT INTO conversations (kind, created_by, dm_pair_key)
             VALUES ('dm', $1, $2) RETURNING id, created_at"
        )
        .bind(me).bind(&pair_key)
        .fetch_one(&mut *tx).await.map_err(AppError::Database)?;

        sqlx::query(
            "INSERT INTO conversation_members (conversation_id, user_id, role)
             VALUES ($1, $2, 'member'), ($1, $3, 'member')"
        )
        .bind(conv.0).bind(me).bind(peer)
        .execute(&mut *tx).await.map_err(AppError::Database)?;
        conv
    };

    tx.commit().await.map_err(AppError::Database)?;

    let row: (Option<String>, Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT wallet_address, username, display_name, avatar_url FROM users WHERE id = $1"
    )
    .bind(peer)
    .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
    let peer_pk: Option<String> = sqlx::query_scalar(
        "SELECT e2ee_public_key FROM users WHERE id = $1"
    )
    .bind(peer)
    .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(ConversationSummary {
        id: conv_id,
        kind: "dm".into(),
        name: None,
        peer_id: Some(peer),
        peer_username: row.1,
        peer_display_name: row.2,
        peer_avatar_url: row.3,
        peer_public_key: peer_pk,
        encrypted_group_key: None,
        wrapper_user_id: None,
        wrapper_public_key: None,
        role: Some("member".into()),
        last_message_at: None,
        created_at,
    })))
}

// ---------- List my conversations ----------

pub async fn list_mine(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<Vec<ConversationSummary>>>> {
    let me = caller_user_id(&state, &auth).await?;

    let rows: Vec<(
        Uuid, String, Option<String>, DateTime<Utc>,
        Option<Uuid>, Option<String>, Option<String>, Option<String>, Option<String>,
        Option<String>, String,
        Option<Uuid>, Option<String>,
        Option<DateTime<Utc>>
    )> = sqlx::query_as(
        "SELECT c.id, c.kind, c.name, c.created_at,
                peer.id AS peer_id, peer.username, peer.display_name,
                peer.avatar_url, peer.e2ee_public_key,
                cm.encrypted_group_key, cm.role,
                cm.wrapper_user_id,
                (SELECT e2ee_public_key FROM users WHERE id = cm.wrapper_user_id) AS wrapper_public_key,
                (SELECT MAX(created_at) FROM messages m WHERE m.conversation_id = c.id)
           FROM conversations c
           JOIN conversation_members cm ON cm.conversation_id = c.id AND cm.user_id = $1
           LEFT JOIN conversation_members cm2
                  ON cm2.conversation_id = c.id AND cm2.user_id <> $1
                 AND c.kind = 'dm'
           LEFT JOIN users peer ON peer.id = cm2.user_id
          WHERE cm.hidden_at IS NULL
          ORDER BY COALESCE(
            (SELECT MAX(created_at) FROM messages m WHERE m.conversation_id = c.id),
            c.created_at
          ) DESC
          LIMIT 200"
    )
    .bind(me)
    .fetch_all(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    let out = rows.into_iter().map(|r| ConversationSummary {
        id: r.0, kind: r.1, name: r.2, created_at: r.3,
        peer_id: r.4, peer_username: r.5, peer_display_name: r.6,
        peer_avatar_url: r.7, peer_public_key: r.8,
        encrypted_group_key: r.9, role: Some(r.10),
        wrapper_user_id: r.11, wrapper_public_key: r.12,
        last_message_at: r.13,
    }).collect();

    Ok(Json(ApiResponse::ok(out)))
}

// ---------- Hide / leave ----------

pub async fn hide(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(conv_id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    let me = caller_user_id(&state, &auth).await?;
    assert_member(state.db.pool(), conv_id, me).await?;
    sqlx::query(
        "UPDATE conversation_members SET hidden_at = NOW() WHERE conversation_id = $1 AND user_id = $2"
    )
    .bind(conv_id).bind(me)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok("hidden")))
}

// ---------- Retention setting ----------

#[derive(Debug, Deserialize)]
pub struct UpdateRetentionRequest {
    pub dm_retention_days: i16,
}

#[derive(Debug, Serialize)]
pub struct RetentionResponse {
    pub dm_retention_days: i16,
}

pub async fn get_retention(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<RetentionResponse>>> {
    let me = caller_user_id(&state, &auth).await?;
    let days: i16 = sqlx::query_scalar(
        "SELECT dm_retention_days FROM users WHERE id = $1"
    )
    .bind(me)
    .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(RetentionResponse { dm_retention_days: days })))
}

pub async fn update_retention(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<UpdateRetentionRequest>,
) -> AppResult<Json<ApiResponse<RetentionResponse>>> {
    if ![1i16, 7, 30].contains(&req.dm_retention_days) {
        return Err(AppError::Validation("dm_retention_days must be 1, 7 or 30".into()));
    }
    let me = caller_user_id(&state, &auth).await?;
    sqlx::query("UPDATE users SET dm_retention_days = $1 WHERE id = $2")
        .bind(req.dm_retention_days).bind(me)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(RetentionResponse { dm_retention_days: req.dm_retention_days })))
}
