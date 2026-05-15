//! Messages — send, list, delete.
//!
//! All ciphertext is opaque to the server. Tip-messages additionally
//! write a `tips` row inside the same transaction so the fee ledger
//! and balance ledger stay consistent with non-DM tips.

use axum::{extract::{Path, Query, State}, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;
use crate::api::conversations::{caller_user_id, assert_member};

const CIPHERTEXT_MAX_LEN: usize = 32_000; // ~24 KB plaintext, plenty for text
const IV_LEN: usize = 24; // base64 of 12 bytes = 16 chars; round up

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub kind: String,         // 'text' | 'tip'  (image lives in a separate multipart endpoint, commit 5)
    pub ciphertext: String,
    pub iv: String,
    pub tip_amount: Option<String>, // only for kind='tip'
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
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
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
    if let Some((peer_id, kind)) = peer {
        if kind == "dm" {
            if crate::api::blocks::either_blocks(state.db.pool(), me, peer_id).await? {
                return Err(AppError::Forbidden("Blocked".into()));
            }
        }
    }

    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;

    let tip_id: Option<Uuid> = if req.kind == "tip" {
        // For tip-messages we run the off-chain tip inside the same tx.
        // Recipient is the *other* DM member (groups: tip not supported in v1).
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

    let m: (Uuid, Option<Uuid>, String, String, String, Option<Uuid>,
            Option<DateTime<Utc>>, DateTime<Utc>, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO messages (conversation_id, sender_id, kind, ciphertext, iv, tip_id)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, sender_id, kind, ciphertext, iv, tip_id, deleted_at, created_at, expires_at"
    )
    .bind(conv_id).bind(me).bind(&req.kind)
    .bind(&req.ciphertext).bind(&req.iv).bind(tip_id)
    .fetch_one(&mut *tx).await.map_err(AppError::Database)?;

    // Lift any soft-hide on both sides — a new message un-hides the
    // conversation for the *sender* and the *recipient(s)* alike.
    sqlx::query(
        "UPDATE conversation_members SET hidden_at = NULL WHERE conversation_id = $1"
    )
    .bind(conv_id)
    .execute(&mut *tx).await.map_err(AppError::Database)?;

    tx.commit().await.map_err(AppError::Database)?;

    // Tip amount echoed back from the tips row for the convenience of the UI.
    let tip_amount: Option<f64> = if let Some(tid) = tip_id {
        sqlx::query_scalar("SELECT amount::float8 FROM tips WHERE id = $1")
            .bind(tid).fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
    } else { None };

    Ok(Json(ApiResponse::ok(MessageDto {
        id: m.0, conversation_id: conv_id,
        sender_id: m.1, kind: m.2, ciphertext: m.3, iv: m.4,
        tip_amount, blob_path: None,
        deleted_at: m.6, created_at: m.7, expires_at: m.8,
    })))
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
        Option<DateTime<Utc>>, DateTime<Utc>, DateTime<Utc>
    )> = if let Some(before) = q.before {
        sqlx::query_as(
            "SELECT m.id, m.sender_id, m.kind, m.ciphertext, m.iv,
                    m.tip_id, t.amount::float8 AS tip_amount, m.blob_path,
                    m.deleted_at, m.created_at, m.expires_at
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
                    m.deleted_at, m.created_at, m.expires_at
               FROM messages m
               LEFT JOIN tips t ON t.id = m.tip_id
              WHERE m.conversation_id = $1
              ORDER BY m.created_at DESC
              LIMIT $2"
        )
        .bind(conv_id).bind(limit)
        .fetch_all(state.db.pool()).await.map_err(AppError::Database)?
    };

    // Reverse so the UI receives chronological order
    let mut out: Vec<MessageDto> = rows.into_iter().map(|r| MessageDto {
        id: r.0, conversation_id: conv_id,
        sender_id: r.1, kind: r.2, ciphertext: r.3, iv: r.4,
        tip_amount: r.6, blob_path: r.7,
        deleted_at: r.8, created_at: r.9, expires_at: r.10,
    }).collect();
    out.reverse();
    Ok(Json(ApiResponse::ok(out)))
}

pub async fn delete_one(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(msg_id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    let me = caller_user_id(&state, &auth).await?;
    // Only the sender can delete; we tombstone (deleted_at + blank ciphertext)
    // so the other side sees "[deleted]" rather than a hole.
    let updated = sqlx::query(
        "UPDATE messages
            SET deleted_at = NOW(), ciphertext = '', iv = ''
          WHERE id = $1 AND sender_id = $2 AND deleted_at IS NULL"
    )
    .bind(msg_id).bind(me)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound("Message not found or not yours".into()));
    }
    Ok(Json(ApiResponse::ok("deleted")))
}
