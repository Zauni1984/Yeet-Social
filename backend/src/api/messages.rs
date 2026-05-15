//! Messages — send, list, delete, image upload + fetch.
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

// ---------- image messages ----------

const IMAGE_BLOB_MAX_BYTES: usize = 12 * 1024 * 1024; // 12 MB ciphertext (~ 8-9 MB image)

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

    // Read iv + ciphertext-blob from multipart.
    let mut iv_b64: Option<String> = None;
    let mut blob_bytes: Option<Vec<u8>> = None;
    while let Some(field) = mp.next_field().await
        .map_err(|e| AppError::Validation(format!("Multipart parse error: {e}")))?
    {
        let name = field.name().map(|s| s.to_string()).unwrap_or_default();
        match name.as_str() {
            "iv" => {
                iv_b64 = Some(field.text().await
                    .map_err(|e| AppError::Validation(format!("iv read: {e}")))?);
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
            _ => {} // ignore other fields
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

    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;
    let m: (Uuid, Option<Uuid>, String, String, String, Option<Uuid>,
            Option<DateTime<Utc>>, DateTime<Utc>, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO messages (conversation_id, sender_id, kind, ciphertext, iv, blob_size_bytes)
         VALUES ($1, $2, 'image', '', $3, $4)
         RETURNING id, sender_id, kind, ciphertext, iv, tip_id, deleted_at, created_at, expires_at"
    )
    .bind(conv_id).bind(me).bind(&iv).bind(blob.len() as i64)
    .fetch_one(&mut *tx).await.map_err(AppError::Database)?;

    // Persist to disk under UPLOADS_DIR/dm-blobs/<conv_id>/<msg_id>.bin
    let dir = dm_blobs_dir(conv_id);
    tokio::fs::create_dir_all(&dir).await
        .map_err(|e| AppError::Internal(format!("mkdir: {e}")))?;
    let path = dir.join(format!("{}.bin", m.0));
    {
        let mut f = tokio::fs::File::create(&path).await
            .map_err(|e| AppError::Internal(format!("create: {e}")))?;
        f.write_all(&blob).await.map_err(|e| AppError::Internal(format!("write: {e}")))?;
        f.flush().await.map_err(|e| AppError::Internal(format!("flush: {e}")))?;
    }
    let rel_path = format!("dm-blobs/{}/{}.bin", conv_id, m.0);
    sqlx::query("UPDATE messages SET blob_path = $2 WHERE id = $1")
        .bind(m.0).bind(&rel_path)
        .execute(&mut *tx).await.map_err(AppError::Database)?;

    sqlx::query(
        "UPDATE conversation_members SET hidden_at = NULL WHERE conversation_id = $1"
    )
    .bind(conv_id)
    .execute(&mut *tx).await.map_err(AppError::Database)?;

    tx.commit().await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(MessageDto {
        id: m.0, conversation_id: conv_id,
        sender_id: m.1, kind: m.2, ciphertext: m.3, iv: m.4,
        tip_amount: None, blob_path: Some(rel_path),
        deleted_at: m.6, created_at: m.7, expires_at: m.8,
    })))
}

pub async fn get_blob(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(msg_id): Path<Uuid>,
) -> Result<Response, AppError> {
    let me = caller_user_id(&state, &auth).await?;
    let row: Option<(Uuid, Option<String>, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT conversation_id, blob_path, deleted_at FROM messages
          WHERE id = $1 AND kind = 'image'"
    )
    .bind(msg_id)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (conv_id, blob_path, deleted_at) = row
        .ok_or_else(|| AppError::NotFound("Message not found".into()))?;
    if deleted_at.is_some() {
        return Err(AppError::NotFound("Message deleted".into()));
    }
    assert_member(state.db.pool(), conv_id, me).await?;
    let rel = blob_path.ok_or_else(|| AppError::NotFound("Blob missing".into()))?;
    // Defense-in-depth path validation: the DB value is server-written,
    // but we still refuse anything trying to escape uploads_dir.
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
    // Capture any on-disk blob path before we tombstone.
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

    // Best-effort blob unlink. Defensive: a malicious blob_path would
    // have had to be injected at INSERT time (we control that), but
    // we still refuse anything that escapes the uploads root.
    if let Some(rel) = blob_path {
        if !rel.contains("..") && !rel.starts_with('/') {
            let full = uploads_dir().join(&rel);
            let _ = tokio::fs::remove_file(&full).await;
        }
    }
    Ok(Json(ApiResponse::ok("deleted")))
}
