//! Messages — send, list, edit, delete, image upload + fetch,
//! delivery + read receipts.
//!
//! All ciphertext is opaque to the server. Tip-messages additionally
//! write a `tips` row inside the same transaction so the fee ledger
//! and balance ledger stay consistent with non-DM tips.
//!
//! Image messages: the client AES-GCM encrypts the binary file under
//! the conversation key (with its own IV) and uploads the resulting
//! ciphertext as an opaque blob via multipart. The server stores the
//! blob on disk under UPLOADS_DIR/dm-blobs/<conv_id>/<msg_id>.bin and
//! gates the GET endpoint on conversation membership.
//!
//! Security additions in the 0031 hardening sprint:
//!   * client_message_id for idempotent retries (unique partial index)
//!   * Two-window per-user-per-conversation rate limit on send
//!   * Edit + delete-for-everyone semantics distinct from delete-for-me
//!   * Delivery + read receipts gated on user preference
//!   * Per-conversation self-destruct timer (computed into expires_at)

use axum::{
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;
use crate::api::conversations::{caller_user_id, assert_member};
use crate::api::uploads::uploads_dir;
use crate::services::rate_limit::{self, RateLimitOutcome};

const CIPHERTEXT_MAX_LEN: usize = 32_000; // ~24 KB plaintext, plenty for text
const IV_LEN: usize = 24; // base64 of 12 bytes = 16 chars; round up
const IMAGE_BLOB_MAX_BYTES: usize = 12 * 1024 * 1024; // 12 MB ciphertext

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub kind: String,         // 'text' | 'tip'
    pub ciphertext: String,
    pub iv: String,
    pub tip_amount: Option<String>, // only for kind='tip'
    /// Client-generated UUID. Retried POSTs with the same value yield
    /// the original message back instead of inserting a duplicate.
    /// Optional for backwards compatibility, but required for
    /// at-least-once delivery semantics by serious clients.
    #[serde(default)]
    pub client_message_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct MessageDto {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub sender_id: Option<Uuid>,
    pub kind: String,
    pub ciphertext: String,
    pub iv: String,
    pub tip_amount: Option<f64>,
    pub blob_path: Option<String>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub deleted_for_all_at: Option<DateTime<Utc>>,
    pub edited_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub client_message_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct ListMessagesQuery {
    pub before: Option<DateTime<Utc>>,
    pub limit: Option<i64>,
}

fn validate_blob(ct: &str, iv: &str) -> AppResult<()> {
    if ct.is_empty() || iv.is_empty() {
        return Err(AppError::Validation("ciphertext + iv required".into()));
    }
    if ct.len() > CIPHERTEXT_MAX_LEN {
        return Err(AppError::Validation("ciphertext too large".into()));
    }
    if iv.len() > IV_LEN {
        return Err(AppError::Validation("iv too large".into()));
    }
    let is_b64 = |c: char| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' || c == '-' || c == '_';
    if !ct.chars().all(is_b64) || !iv.chars().all(is_b64) {
        return Err(AppError::Validation("ciphertext + iv must be base64".into()));
    }
    Ok(())
}

/// Two-window per-(user, conversation) cap on message send:
///   * burst:    30 msgs / 30 s   (mass-DM defence)
///   * sustained: 600 msgs / hour (sustained-volume defence)
///
/// Plus a global per-user envelope of 4000 msgs / hour across all
/// conversations to catch broadcast-style spam that spreads thin.
async fn rate_limit_send(state: &AppState, user_id: Uuid, conv_id: Uuid) -> AppResult<()> {
    let per_conv = format!("{user_id}:{conv_id}");
    match rate_limit::check_two_window(
        &state.cache, "msg_send_conv", &per_conv,
        30, 30,
        3600, 600,
    ).await {
        RateLimitOutcome::Allowed => {}
        _ => return Err(AppError::RateLimited),
    }
    let principal = user_id.to_string();
    match rate_limit::check_two_window(
        &state.cache, "msg_send_global", &principal,
        30, 100,
        3600, 4000,
    ).await {
        RateLimitOutcome::Allowed => Ok(()),
        _ => Err(AppError::RateLimited),
    }
}

/// Compute the message's expires_at given the conversation's
/// self-destruct timer (if any). Hard-capped to 30 days regardless of
/// per-conversation setting so the cleanup invariant doesn't drift.
async fn compute_expires_at(state: &AppState, conv_id: Uuid) -> AppResult<DateTime<Utc>> {
    let secs: Option<i32> = sqlx::query_scalar(
        "SELECT self_destruct_seconds FROM conversations WHERE id = $1"
    )
    .bind(conv_id)
    .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
    let default_30d = chrono::Duration::days(30);
    let dur = match secs {
        Some(s) if s > 0 => {
            let secs_capped = s.min(30 * 24 * 3600) as i64;
            chrono::Duration::seconds(secs_capped).min(default_30d)
        }
        _ => default_30d,
    };
    Ok(Utc::now() + dur)
}

pub async fn send(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(conv_id): Path<Uuid>,
    Json(req): Json<SendMessageRequest>,
) -> AppResult<Json<ApiResponse<MessageDto>>> {
    let me = caller_user_id(&state, &auth).await?;
    assert_member(state.db.pool(), conv_id, me).await?;
    validate_blob(&req.ciphertext, &req.iv)?;

    if !["text", "tip"].contains(&req.kind.as_str()) {
        return Err(AppError::Validation("kind must be text or tip".into()));
    }

    rate_limit_send(&state, me, conv_id).await?;

    // Idempotency check FIRST. A retry of the same client_message_id
    // returns the existing row without re-running any side effects
    // (notify, tip transfer, hidden_at toggle). This guards against
    // network retries triggering duplicate tip-transfers.
    if let Some(cmid) = req.client_message_id {
        let existing: Option<MessageRow> = sqlx::query_as::<_, MessageRow>(SELECT_MESSAGE_BY_IDEMP)
            .bind(me).bind(conv_id).bind(cmid)
            .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
        if let Some(row) = existing {
            return Ok(Json(ApiResponse::ok(row.into_dto(conv_id))));
        }
    }

    // For 1:1 DMs: refuse if either party blocked the other.
    let peer: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT cm.user_id, c.kind
           FROM conversation_members cm
           JOIN conversations c ON c.id = cm.conversation_id
          WHERE cm.conversation_id = $1 AND cm.user_id <> $2
          LIMIT 1"
    )
    .bind(conv_id).bind(me)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    if let Some((peer_id, kind)) = &peer {
        if kind == "dm"
            && crate::api::blocks::either_blocks(state.db.pool(), me, *peer_id).await?
        {
            return Err(AppError::Forbidden("Blocked".into()));
        }
    }

    let expires_at = compute_expires_at(&state, conv_id).await?;
    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;

    let tip_id: Option<Uuid> = if req.kind == "tip" {
        let kind: String = sqlx::query_scalar("SELECT kind FROM conversations WHERE id = $1")
            .bind(conv_id).fetch_one(&mut *tx).await.map_err(AppError::Database)?;
        if kind != "dm" {
            return Err(AppError::Validation("Tips in groups are not supported in v1".into()));
        }
        let recipient: Uuid = sqlx::query_scalar(
            "SELECT user_id FROM conversation_members
              WHERE conversation_id = $1 AND user_id <> $2"
        )
        .bind(conv_id).bind(me)
        .fetch_one(&mut *tx).await.map_err(AppError::Database)?;
        let amount = req.tip_amount.as_deref()
            .ok_or_else(|| AppError::Validation("tip_amount required for kind=tip".into()))?;
        let id = crate::api::tips::send_tip_tx(
            &mut tx, me, recipient, None, amount, "YEET", None
        ).await?;
        Some(id)
    } else {
        None
    };

    let m: MessageRow = match sqlx::query_as::<_, MessageRow>(
        "INSERT INTO messages
            (conversation_id, sender_id, kind, ciphertext, iv, tip_id,
             client_message_id, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING id, sender_id, kind, ciphertext, iv, tip_id,
                   blob_path, deleted_at, deleted_for_all_at, edited_at,
                   created_at, expires_at, client_message_id"
    )
    .bind(conv_id).bind(me).bind(&req.kind)
    .bind(&req.ciphertext).bind(&req.iv).bind(tip_id)
    .bind(req.client_message_id).bind(expires_at)
    .fetch_one(&mut *tx).await
    {
        Ok(row) => row,
        Err(sqlx::Error::Database(e)) if e.constraint() == Some("uq_messages_idempotency") => {
            // Lost the race with a concurrent same-idempotency POST.
            // Return the row that won.
            tx.rollback().await.map_err(AppError::Database)?;
            let existing: MessageRow = sqlx::query_as::<_, MessageRow>(SELECT_MESSAGE_BY_IDEMP)
                .bind(me).bind(conv_id).bind(req.client_message_id)
                .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
            return Ok(Json(ApiResponse::ok(existing.into_dto(conv_id))));
        }
        Err(e) => return Err(AppError::Database(e)),
    };

    sqlx::query(
        "UPDATE conversation_members SET hidden_at = NULL WHERE conversation_id = $1"
    )
    .bind(conv_id)
    .execute(&mut *tx).await.map_err(AppError::Database)?;

    tx.commit().await.map_err(AppError::Database)?;

    // Notify recipients — skip anyone who muted this conversation.
    let recipients: Vec<Uuid> = sqlx::query_scalar(
        "SELECT user_id FROM conversation_members
          WHERE conversation_id = $1
            AND user_id <> $2
            AND (muted_until IS NULL OR muted_until < NOW())"
    )
    .bind(conv_id).bind(me)
    .fetch_all(state.db.pool()).await.unwrap_or_default();
    let actor_name = sqlx::query_scalar::<_, Option<String>>(
        "SELECT COALESCE(display_name, username) FROM users WHERE id = $1"
    ).bind(me).fetch_optional(state.db.pool()).await
     .ok().flatten().flatten().unwrap_or_else(|| "Someone".into());
    let body = if req.kind == "tip" {
        format!("{} sent you YEET in a private message", actor_name)
    } else {
        format!("New message from {}", actor_name)
    };
    for r_id in recipients {
        crate::api::notifications::notify(
            state.db.pool(), r_id, Some(me),
            "dm_message", &body, None,
        ).await;
    }

    Ok(Json(ApiResponse::ok(m.into_dto(conv_id))))
}

const SELECT_MESSAGE_BY_IDEMP: &str =
    "SELECT id, sender_id, kind, ciphertext, iv, tip_id,
            blob_path, deleted_at, deleted_for_all_at, edited_at,
            created_at, expires_at, client_message_id
       FROM messages
      WHERE sender_id = $1 AND conversation_id = $2 AND client_message_id = $3";

#[derive(sqlx::FromRow)]
struct MessageRow {
    id: Uuid,
    sender_id: Option<Uuid>,
    kind: String,
    ciphertext: String,
    iv: String,
    tip_id: Option<Uuid>,
    blob_path: Option<String>,
    deleted_at: Option<DateTime<Utc>>,
    deleted_for_all_at: Option<DateTime<Utc>>,
    edited_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    client_message_id: Option<Uuid>,
}

impl MessageRow {
    fn into_dto(self, conv_id: Uuid) -> MessageDto {
        MessageDto {
            id: self.id, conversation_id: conv_id,
            sender_id: self.sender_id, kind: self.kind,
            ciphertext: self.ciphertext, iv: self.iv,
            // tip_amount is hydrated separately by list() when needed;
            // send() can leave it None because the client already has
            // the tip context locally.
            tip_amount: None,
            blob_path: self.blob_path,
            deleted_at: self.deleted_at,
            deleted_for_all_at: self.deleted_for_all_at,
            edited_at: self.edited_at,
            created_at: self.created_at,
            expires_at: self.expires_at,
            client_message_id: self.client_message_id,
        }
    }
}

pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(conv_id): Path<Uuid>,
    Query(q): Query<ListMessagesQuery>,
) -> AppResult<Json<ApiResponse<Vec<MessageDto>>>> {
    let me = caller_user_id(&state, &auth).await?;
    assert_member(state.db.pool(), conv_id, me).await?;
    let limit = q.limit.unwrap_or(50).clamp(1, 200);

    let rows: Vec<(
        Uuid, Option<Uuid>, String, String, String,
        Option<Uuid>, Option<f64>, Option<String>,
        Option<DateTime<Utc>>, Option<DateTime<Utc>>, Option<DateTime<Utc>>,
        DateTime<Utc>, DateTime<Utc>, Option<Uuid>
    )> = if let Some(before) = q.before {
        sqlx::query_as(
            "SELECT m.id, m.sender_id, m.kind, m.ciphertext, m.iv,
                    m.tip_id, t.amount::float8 AS tip_amount, m.blob_path,
                    m.deleted_at, m.deleted_for_all_at, m.edited_at,
                    m.created_at, m.expires_at, m.client_message_id
               FROM messages m
               LEFT JOIN tips t ON t.id = m.tip_id
              WHERE m.conversation_id = $1 AND m.created_at < $2
              ORDER BY m.created_at DESC
              LIMIT $3"
        )
        .bind(conv_id).bind(before).bind(limit)
        .fetch_all(state.db.pool()).await.map_err(AppError::Database)?
    } else {
        sqlx::query_as(
            "SELECT m.id, m.sender_id, m.kind, m.ciphertext, m.iv,
                    m.tip_id, t.amount::float8 AS tip_amount, m.blob_path,
                    m.deleted_at, m.deleted_for_all_at, m.edited_at,
                    m.created_at, m.expires_at, m.client_message_id
               FROM messages m
               LEFT JOIN tips t ON t.id = m.tip_id
              WHERE m.conversation_id = $1
              ORDER BY m.created_at DESC
              LIMIT $2"
        )
        .bind(conv_id).bind(limit)
        .fetch_all(state.db.pool()).await.map_err(AppError::Database)?
    };

    // Reverse so the UI receives chronological order. Strip ciphertext
    // from rows that were delete-for-everyone'd so a delete is final
    // even from the server's perspective.
    let mut out: Vec<MessageDto> = rows.into_iter().map(|r| {
        let deleted_for_all = r.9.is_some();
        MessageDto {
            id: r.0, conversation_id: conv_id,
            sender_id: r.1, kind: r.2,
            ciphertext: if deleted_for_all { String::new() } else { r.3 },
            iv: if deleted_for_all { String::new() } else { r.4 },
            tip_amount: r.6,
            blob_path: if deleted_for_all { None } else { r.7 },
            deleted_at: r.8, deleted_for_all_at: r.9, edited_at: r.10,
            created_at: r.11, expires_at: r.12, client_message_id: r.13,
        }
    }).collect();
    out.reverse();
    Ok(Json(ApiResponse::ok(out)))
}

// ─── Edit message ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct EditMessageRequest {
    pub ciphertext: String,
    pub iv: String,
}

pub async fn edit_message(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(msg_id): Path<Uuid>,
    Json(req): Json<EditMessageRequest>,
) -> AppResult<Json<ApiResponse<MessageDto>>> {
    validate_blob(&req.ciphertext, &req.iv)?;
    let me = caller_user_id(&state, &auth).await?;

    // Eligibility: caller is the sender, message is text (no editing
    // tips or images), within an edit window (15 min). Tombstoned
    // messages and delete-for-all messages refuse the edit.
    let row: Option<(Uuid, String, DateTime<Utc>, Option<DateTime<Utc>>, Option<DateTime<Utc>>)> =
        sqlx::query_as(
            "SELECT conversation_id, kind, created_at, deleted_at, deleted_for_all_at
               FROM messages WHERE id = $1 AND sender_id = $2"
        )
        .bind(msg_id).bind(me)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (conv_id, kind, created_at, del_me, del_all) = row
        .ok_or_else(|| AppError::NotFound("Message not found or not yours".into()))?;
    if del_me.is_some() || del_all.is_some() {
        return Err(AppError::Validation("Message already deleted".into()));
    }
    if kind != "text" {
        return Err(AppError::Validation("Only text messages are editable".into()));
    }
    if Utc::now() - created_at > chrono::Duration::minutes(15) {
        return Err(AppError::Validation("Edit window (15 min) has passed".into()));
    }

    // Re-check the tombstone columns in the same UPDATE so a
    // delete-for-all that landed between the SELECT above and this
    // UPDATE can't be "un-deleted" by an edit. fetch_optional + None
    // means the race went the other way and we report a generic 404
    // (which is correct — the message no longer exists for editing).
    let m: Option<MessageRow> = sqlx::query_as::<_, MessageRow>(
        "UPDATE messages
            SET ciphertext = $2, iv = $3, edited_at = NOW()
          WHERE id = $1
            AND sender_id = $4
            AND deleted_at IS NULL
            AND deleted_for_all_at IS NULL
         RETURNING id, sender_id, kind, ciphertext, iv, tip_id,
                   blob_path, deleted_at, deleted_for_all_at, edited_at,
                   created_at, expires_at, client_message_id"
    )
    .bind(msg_id).bind(&req.ciphertext).bind(&req.iv).bind(me)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let m = m.ok_or_else(|| AppError::NotFound("Message not found or not yours".into()))?;
    Ok(Json(ApiResponse::ok(m.into_dto(conv_id))))
}

// ─── Delete-for-everyone ───────────────────────────────────────────────

/// Unsend a message for every participant. The sender can do this up
/// to a generous 24h window after creation. We tombstone the row in
/// `deleted_for_all_at`, scrub ciphertext + IV on the same update, and
/// unlink any on-disk blob. list() filters delete-for-all rows out of
/// the ciphertext payload going forward; existing recipients' clients
/// reconcile on next fetch.
pub async fn delete_for_all(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(msg_id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    let me = caller_user_id(&state, &auth).await?;

    let row: Option<(String, DateTime<Utc>, Option<String>)> = sqlx::query_as(
        "SELECT kind, created_at, blob_path
           FROM messages
          WHERE id = $1 AND sender_id = $2
            AND deleted_for_all_at IS NULL"
    )
    .bind(msg_id).bind(me)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (_kind, created_at, blob_path) = row
        .ok_or_else(|| AppError::NotFound("Message not found or not yours".into()))?;
    if Utc::now() - created_at > chrono::Duration::hours(24) {
        return Err(AppError::Validation("Delete-for-everyone window has passed".into()));
    }

    // Refuse the UPDATE if a concurrent caller already tombstoned it
    // (re-checking sender_id + the timestamp keeps the operation
    // idempotent and TOCTOU-safe).
    sqlx::query(
        "UPDATE messages
            SET deleted_for_all_at = NOW(),
                ciphertext = '', iv = '',
                blob_path = NULL
          WHERE id = $1 AND sender_id = $2
            AND deleted_for_all_at IS NULL"
    )
    .bind(msg_id).bind(me)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;

    if let Some(rel) = blob_path {
        if !rel.contains("..") && !rel.starts_with('/') {
            let full = uploads_dir().join(&rel);
            let _ = tokio::fs::remove_file(&full).await;
        }
    }
    Ok(Json(ApiResponse::ok("deleted_for_all")))
}

// ─── Delivery + read receipts ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct MarkReceiptsRequest {
    pub message_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct ReceiptCounts { pub recorded: i64 }

/// Caller confirms they pulled these messages from the server. The
/// caller must be a member of every message's conversation; non-
/// matches are silently skipped so a single bad id in a batch doesn't
/// fail the whole call.
pub async fn mark_delivered(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<MarkReceiptsRequest>,
) -> AppResult<Json<ApiResponse<ReceiptCounts>>> {
    let me = caller_user_id(&state, &auth).await?;
    if req.message_ids.is_empty() { return Ok(Json(ApiResponse::ok(ReceiptCounts { recorded: 0 }))); }
    if req.message_ids.len() > 500 {
        return Err(AppError::Validation("too many message ids in one call".into()));
    }
    let result = sqlx::query(
        "WITH eligible AS (
           SELECT m.id
             FROM messages m
             JOIN conversation_members cm
               ON cm.conversation_id = m.conversation_id AND cm.user_id = $2
            WHERE m.id = ANY($1)
              AND m.sender_id IS DISTINCT FROM $2
         )
         INSERT INTO message_deliveries (message_id, user_id)
         SELECT id, $2 FROM eligible
         ON CONFLICT (message_id, user_id) DO NOTHING"
    )
    .bind(&req.message_ids).bind(me)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(ReceiptCounts { recorded: result.rows_affected() as i64 })))
}

/// Caller confirms the user actually saw these messages. Gated on the
/// user's read_receipts_enabled flag: if they opted out, the rows are
/// silently dropped so they don't expose viewing to senders either.
pub async fn mark_read(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<MarkReceiptsRequest>,
) -> AppResult<Json<ApiResponse<ReceiptCounts>>> {
    let me = caller_user_id(&state, &auth).await?;
    if req.message_ids.is_empty() { return Ok(Json(ApiResponse::ok(ReceiptCounts { recorded: 0 }))); }
    if req.message_ids.len() > 500 {
        return Err(AppError::Validation("too many message ids in one call".into()));
    }

    let receipts_on: bool = sqlx::query_scalar(
        "SELECT read_receipts_enabled FROM users WHERE id = $1"
    )
    .bind(me)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
    .unwrap_or(true);
    if !receipts_on {
        return Ok(Json(ApiResponse::ok(ReceiptCounts { recorded: 0 })));
    }

    let result = sqlx::query(
        "WITH eligible AS (
           SELECT m.id
             FROM messages m
             JOIN conversation_members cm
               ON cm.conversation_id = m.conversation_id AND cm.user_id = $2
            WHERE m.id = ANY($1)
              AND m.sender_id IS DISTINCT FROM $2
         )
         INSERT INTO message_read_receipts (message_id, user_id)
         SELECT id, $2 FROM eligible
         ON CONFLICT (message_id, user_id) DO NOTHING"
    )
    .bind(&req.message_ids).bind(me)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(ReceiptCounts { recorded: result.rows_affected() as i64 })))
}

#[derive(Debug, Serialize)]
pub struct ReceiptStateRow {
    pub message_id: Uuid,
    pub delivered_to: Vec<Uuid>,
    pub read_by: Vec<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct ReceiptStateQuery { pub message_ids: String /* comma-separated */ }

/// Read the receipt state for a batch of messages. Caller must be the
/// sender of every message in the batch (the only person who needs
/// this data); non-matches are filtered. We honour the readers'
/// read_receipts_enabled preference by emitting only deliveries for
/// users who turned receipts off — the sender never learns whether
/// such a user "read" anything.
pub async fn get_receipts(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<ReceiptStateQuery>,
) -> AppResult<Json<ApiResponse<Vec<ReceiptStateRow>>>> {
    let me = caller_user_id(&state, &auth).await?;
    let ids: Vec<Uuid> = q.message_ids.split(',')
        .filter_map(|s| Uuid::parse_str(s.trim()).ok())
        .take(500)
        .collect();
    if ids.is_empty() { return Ok(Json(ApiResponse::ok(Vec::new()))); }

    let deliveries: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT d.message_id, d.user_id
           FROM message_deliveries d
           JOIN messages m ON m.id = d.message_id
          WHERE d.message_id = ANY($1) AND m.sender_id = $2"
    )
    .bind(&ids).bind(me)
    .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;

    let reads: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT r.message_id, r.user_id
           FROM message_read_receipts r
           JOIN messages m ON m.id = r.message_id
           JOIN users u ON u.id = r.user_id
          WHERE r.message_id = ANY($1) AND m.sender_id = $2
            AND u.read_receipts_enabled = TRUE"
    )
    .bind(&ids).bind(me)
    .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;

    use std::collections::HashMap;
    let mut by_msg: HashMap<Uuid, ReceiptStateRow> = HashMap::new();
    for id in &ids {
        by_msg.insert(*id, ReceiptStateRow {
            message_id: *id, delivered_to: Vec::new(), read_by: Vec::new(),
        });
    }
    for (mid, uid) in deliveries {
        if let Some(r) = by_msg.get_mut(&mid) { r.delivered_to.push(uid); }
    }
    for (mid, uid) in reads {
        if let Some(r) = by_msg.get_mut(&mid) { r.read_by.push(uid); }
    }
    Ok(Json(ApiResponse::ok(by_msg.into_values().collect())))
}

// ─── Image messages (unchanged crypto, hardened with rate-limit + idempotency) ──

fn dm_blobs_dir(conv_id: Uuid) -> PathBuf {
    uploads_dir().join("dm-blobs").join(conv_id.to_string())
}

pub async fn upload_image(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(conv_id): Path<Uuid>,
    mut mp: Multipart,
) -> AppResult<Json<ApiResponse<MessageDto>>> {
    let me = caller_user_id(&state, &auth).await?;
    assert_member(state.db.pool(), conv_id, me).await?;
    rate_limit_send(&state, me, conv_id).await?;

    let kind_row: String = sqlx::query_scalar("SELECT kind FROM conversations WHERE id = $1")
        .bind(conv_id).fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
    if kind_row == "dm" {
        let peer: Option<Uuid> = sqlx::query_scalar(
            "SELECT user_id FROM conversation_members
              WHERE conversation_id = $1 AND user_id <> $2 LIMIT 1"
        ).bind(conv_id).bind(me).fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
        if let Some(p) = peer {
            if crate::api::blocks::either_blocks(state.db.pool(), me, p).await? {
                return Err(AppError::Forbidden("Blocked".into()));
            }
        }
    }

    // Multipart: iv + file + optional client_message_id.
    let mut iv_b64: Option<String> = None;
    let mut blob_bytes: Option<Vec<u8>> = None;
    let mut client_message_id: Option<Uuid> = None;
    while let Some(field) = mp.next_field().await
        .map_err(|e| AppError::Validation(format!("Multipart parse error: {e}")))?
    {
        let name = field.name().map(|s| s.to_string()).unwrap_or_default();
        match name.as_str() {
            "iv" => {
                iv_b64 = Some(field.text().await
                    .map_err(|e| AppError::Validation(format!("iv read: {e}")))?);
            }
            "client_message_id" => {
                let txt = field.text().await
                    .map_err(|e| AppError::Validation(format!("cmid read: {e}")))?;
                client_message_id = Uuid::parse_str(txt.trim()).ok();
            }
            "file" => {
                let b = field.bytes().await
                    .map_err(|e| AppError::Validation(format!("file read: {e}")))?;
                if b.len() > IMAGE_BLOB_MAX_BYTES {
                    return Err(AppError::Validation("Image too large".into()));
                }
                if b.is_empty() {
                    return Err(AppError::Validation("Empty file".into()));
                }
                blob_bytes = Some(b.to_vec());
            }
            _ => {}
        }
    }
    let iv = iv_b64.ok_or_else(|| AppError::Validation("iv field required".into()))?;
    let blob = blob_bytes.ok_or_else(|| AppError::Validation("file field required".into()))?;
    if iv.is_empty() || iv.len() > IV_LEN {
        return Err(AppError::Validation("invalid iv length".into()));
    }
    let is_b64 = |c: char| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' || c == '-' || c == '_';
    if !iv.chars().all(is_b64) {
        return Err(AppError::Validation("iv must be base64".into()));
    }

    // Idempotency check for image uploads too.
    if let Some(cmid) = client_message_id {
        let existing: Option<MessageRow> = sqlx::query_as::<_, MessageRow>(SELECT_MESSAGE_BY_IDEMP)
            .bind(me).bind(conv_id).bind(cmid)
            .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
        if let Some(row) = existing {
            return Ok(Json(ApiResponse::ok(row.into_dto(conv_id))));
        }
    }

    let expires_at = compute_expires_at(&state, conv_id).await?;
    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;
    let m: MessageRow = sqlx::query_as::<_, MessageRow>(
        "INSERT INTO messages
            (conversation_id, sender_id, kind, ciphertext, iv,
             blob_size_bytes, client_message_id, expires_at)
         VALUES ($1, $2, 'image', '', $3, $4, $5, $6)
         RETURNING id, sender_id, kind, ciphertext, iv, tip_id,
                   blob_path, deleted_at, deleted_for_all_at, edited_at,
                   created_at, expires_at, client_message_id"
    )
    .bind(conv_id).bind(me).bind(&iv).bind(blob.len() as i64)
    .bind(client_message_id).bind(expires_at)
    .fetch_one(&mut *tx).await.map_err(AppError::Database)?;

    let dir = dm_blobs_dir(conv_id);
    tokio::fs::create_dir_all(&dir).await
        .map_err(|e| AppError::Internal(format!("mkdir: {e}")))?;
    let path = dir.join(format!("{}.bin", m.id));
    {
        let mut f = tokio::fs::File::create(&path).await
            .map_err(|e| AppError::Internal(format!("create: {e}")))?;
        f.write_all(&blob).await.map_err(|e| AppError::Internal(format!("write: {e}")))?;
        f.flush().await.map_err(|e| AppError::Internal(format!("flush: {e}")))?;
    }
    let rel_path = format!("dm-blobs/{}/{}.bin", conv_id, m.id);
    sqlx::query("UPDATE messages SET blob_path = $2 WHERE id = $1")
        .bind(m.id).bind(&rel_path)
        .execute(&mut *tx).await.map_err(AppError::Database)?;

    sqlx::query(
        "UPDATE conversation_members SET hidden_at = NULL WHERE conversation_id = $1"
    )
    .bind(conv_id)
    .execute(&mut *tx).await.map_err(AppError::Database)?;

    tx.commit().await.map_err(AppError::Database)?;

    let mut dto = m.into_dto(conv_id);
    dto.blob_path = Some(rel_path);
    Ok(Json(ApiResponse::ok(dto)))
}

pub async fn get_blob(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(msg_id): Path<Uuid>,
) -> Result<Response, AppError> {
    let me = caller_user_id(&state, &auth).await?;
    let row: Option<(Uuid, Option<String>, Option<DateTime<Utc>>, Option<DateTime<Utc>>)> =
        sqlx::query_as(
            "SELECT conversation_id, blob_path, deleted_at, deleted_for_all_at
               FROM messages
              WHERE id = $1 AND kind = 'image'"
        )
        .bind(msg_id)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (conv_id, blob_path, deleted_at, deleted_for_all_at) = row
        .ok_or_else(|| AppError::NotFound("Message not found".into()))?;
    if deleted_at.is_some() || deleted_for_all_at.is_some() {
        return Err(AppError::NotFound("Message deleted".into()));
    }
    assert_member(state.db.pool(), conv_id, me).await?;
    let rel = blob_path.ok_or_else(|| AppError::NotFound("Blob missing".into()))?;
    if rel.contains("..") || rel.starts_with('/') {
        return Err(AppError::NotFound("Blob missing".into()));
    }
    let path = uploads_dir().join(&rel);
    let bytes = tokio::fs::read(&path).await
        .map_err(|_| AppError::NotFound("Blob missing".into()))?;
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    Ok((StatusCode::OK, headers, Body::from(bytes)).into_response())
}

pub async fn delete_one(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(msg_id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    let me = caller_user_id(&state, &auth).await?;
    let row: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT kind, blob_path FROM messages
          WHERE id = $1 AND sender_id = $2 AND deleted_at IS NULL"
    )
    .bind(msg_id).bind(me)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (_kind, blob_path) = row.ok_or_else(||
        AppError::NotFound("Message not found or not yours".into()))?;

    sqlx::query(
        "UPDATE messages
            SET deleted_at = NOW(), ciphertext = '', iv = '', blob_path = NULL
          WHERE id = $1 AND sender_id = $2"
    )
    .bind(msg_id).bind(me)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;

    if let Some(rel) = blob_path {
        if !rel.contains("..") && !rel.starts_with('/') {
            let full = uploads_dir().join(&rel);
            let _ = tokio::fs::remove_file(&full).await;
        }
    }
    Ok(Json(ApiResponse::ok("deleted")))
}
