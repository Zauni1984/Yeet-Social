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
    /// Per-member UX state surfaced on list. Defaults preserve the
    /// previous behaviour: not muted, not archived, no per-conv timer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted_until: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_destruct_seconds: Option<i32>,
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
        muted_until: None,
        archived_at: None,
        self_destruct_seconds: None,
    })))
}

// ---------- List my conversations ----------

#[derive(Debug, Deserialize)]
pub struct ListConvsQuery {
    /// When true, returns archived conversations instead of active.
    /// Mute and self-destruct state is always surfaced regardless.
    #[serde(default)]
    pub archived: Option<bool>,
}

#[derive(sqlx::FromRow)]
struct ConversationListRow {
    id: Uuid,
    kind: String,
    name: Option<String>,
    created_at: DateTime<Utc>,
    peer_id: Option<Uuid>,
    peer_username: Option<String>,
    peer_display_name: Option<String>,
    peer_avatar_url: Option<String>,
    peer_public_key: Option<String>,
    encrypted_group_key: Option<String>,
    role: String,
    wrapper_user_id: Option<Uuid>,
    wrapper_public_key: Option<String>,
    last_message_at: Option<DateTime<Utc>>,
    muted_until: Option<DateTime<Utc>>,
    archived_at: Option<DateTime<Utc>>,
    self_destruct_seconds: Option<i32>,
}

pub async fn list_mine(
    State(state): State<AppState>,
    auth: AuthUser,
    axum::extract::Query(q): axum::extract::Query<ListConvsQuery>,
) -> AppResult<Json<ApiResponse<Vec<ConversationSummary>>>> {
    let me = caller_user_id(&state, &auth).await?;
    let archived = q.archived.unwrap_or(false);

    let rows: Vec<ConversationListRow> = sqlx::query_as(
        "SELECT c.id, c.kind, c.name, c.created_at,
                peer.id AS peer_id, peer.username AS peer_username,
                peer.display_name AS peer_display_name,
                peer.avatar_url AS peer_avatar_url,
                peer.e2ee_public_key AS peer_public_key,
                cm.encrypted_group_key, cm.role,
                cm.wrapper_user_id,
                (SELECT e2ee_public_key FROM users WHERE id = cm.wrapper_user_id) AS wrapper_public_key,
                (SELECT MAX(created_at) FROM messages m WHERE m.conversation_id = c.id) AS last_message_at,
                cm.muted_until, cm.archived_at, c.self_destruct_seconds
           FROM conversations c
           JOIN conversation_members cm ON cm.conversation_id = c.id AND cm.user_id = $1
           LEFT JOIN conversation_members cm2
                  ON cm2.conversation_id = c.id AND cm2.user_id <> $1
                 AND c.kind = 'dm'
           LEFT JOIN users peer ON peer.id = cm2.user_id
          WHERE cm.hidden_at IS NULL
            AND (CASE WHEN $2 THEN cm.archived_at IS NOT NULL
                                 ELSE cm.archived_at IS NULL END)
          ORDER BY COALESCE(
            (SELECT MAX(created_at) FROM messages m WHERE m.conversation_id = c.id),
            c.created_at
          ) DESC
          LIMIT 200"
    )
    .bind(me).bind(archived)
    .fetch_all(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    let out = rows.into_iter().map(|r| ConversationSummary {
        id: r.id, kind: r.kind, name: r.name, created_at: r.created_at,
        peer_id: r.peer_id, peer_username: r.peer_username,
        peer_display_name: r.peer_display_name,
        peer_avatar_url: r.peer_avatar_url, peer_public_key: r.peer_public_key,
        encrypted_group_key: r.encrypted_group_key,
        role: Some(r.role),
        wrapper_user_id: r.wrapper_user_id,
        wrapper_public_key: r.wrapper_public_key,
        last_message_at: r.last_message_at,
        muted_until: r.muted_until,
        archived_at: r.archived_at,
        self_destruct_seconds: r.self_destruct_seconds,
    }).collect();

    Ok(Json(ApiResponse::ok(out)))
}

// ---------- Mute / archive / self-destruct ----------

#[derive(Debug, Deserialize)]
pub struct MuteRequest {
    /// 0 = unmute, else seconds-from-now until mute expires. Hard
    /// capped at one year to avoid "permanent mute" stale state.
    pub seconds: i64,
}

pub async fn mute(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(conv_id): Path<Uuid>,
    Json(req): Json<MuteRequest>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    let me = caller_user_id(&state, &auth).await?;
    assert_member(state.db.pool(), conv_id, me).await?;
    let secs = req.seconds.clamp(0, 365 * 24 * 3600);
    if secs == 0 {
        sqlx::query(
            "UPDATE conversation_members SET muted_until = NULL
              WHERE conversation_id = $1 AND user_id = $2"
        )
        .bind(conv_id).bind(me)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;
        return Ok(Json(ApiResponse::ok("unmuted")));
    }
    let until = Utc::now() + chrono::Duration::seconds(secs);
    sqlx::query(
        "UPDATE conversation_members SET muted_until = $3
          WHERE conversation_id = $1 AND user_id = $2"
    )
    .bind(conv_id).bind(me).bind(until)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok("muted")))
}

#[derive(Debug, Deserialize)]
pub struct ArchiveRequest { pub archive: bool }

pub async fn archive(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(conv_id): Path<Uuid>,
    Json(req): Json<ArchiveRequest>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    let me = caller_user_id(&state, &auth).await?;
    assert_member(state.db.pool(), conv_id, me).await?;
    if req.archive {
        sqlx::query(
            "UPDATE conversation_members SET archived_at = NOW()
              WHERE conversation_id = $1 AND user_id = $2"
        )
        .bind(conv_id).bind(me)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;
        Ok(Json(ApiResponse::ok("archived")))
    } else {
        sqlx::query(
            "UPDATE conversation_members SET archived_at = NULL
              WHERE conversation_id = $1 AND user_id = $2"
        )
        .bind(conv_id).bind(me)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;
        Ok(Json(ApiResponse::ok("unarchived")))
    }
}

#[derive(Debug, Deserialize)]
pub struct SelfDestructRequest {
    /// NULL or 0 disables the per-conversation timer; otherwise
    /// 5-2_592_000 (5s to 30d). Any member can set this for DMs; only
    /// admins can set it for groups. The cap is enforced by the DB
    /// CHECK constraint as a second line of defence.
    pub seconds: Option<i32>,
}

pub async fn set_self_destruct(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(conv_id): Path<Uuid>,
    Json(req): Json<SelfDestructRequest>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    let me = caller_user_id(&state, &auth).await?;
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT c.kind, cm.role
           FROM conversations c
           JOIN conversation_members cm
             ON cm.conversation_id = c.id AND cm.user_id = $2
          WHERE c.id = $1"
    )
    .bind(conv_id).bind(me)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (kind, role) = row.ok_or_else(|| AppError::Forbidden("Not a member".into()))?;
    if kind == "group" && role != "admin" {
        return Err(AppError::Forbidden("Only group admins can change self-destruct".into()));
    }

    let val = match req.seconds {
        None | Some(0) => None,
        Some(s) if (5..=30 * 24 * 3600).contains(&s) => Some(s),
        _ => return Err(AppError::Validation("seconds must be 5..=2_592_000 or null".into())),
    };
    sqlx::query("UPDATE conversations SET self_destruct_seconds = $2 WHERE id = $1")
        .bind(conv_id).bind(val)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(match val { Some(_) => "set", None => "cleared" })))
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
