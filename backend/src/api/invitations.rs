//! Group conversations + invitations.
//!
//! Group creation includes a per-member envelope (group_key encrypted
//! with each member's pubkey-derived AES-GCM key). Invitations carry
//! the same envelope; on accept the row is promoted to a
//! conversation_members row and the envelope is copied.
//!
//! When a member is kicked, every remaining envelope is NULL'd so the
//! client knows to derive a new group key and POST it via rotate_key.
//! The kicked member's old key remains valid for messages they already
//! decrypted; this is the v1 limitation acknowledged in the plan.

use axum::{extract::{Path, State}, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;
use crate::api::conversations::{caller_user_id, resolve_user, assert_member};

// ---------- Create group ----------

#[derive(Debug, Deserialize)]
pub struct CreateGroupRequest {
    pub name: String,
    pub members: Vec<GroupMemberInit>,
}

#[derive(Debug, Deserialize)]
pub struct GroupMemberInit {
    pub user_id: Uuid,
    pub encrypted_group_key: String,
}

#[derive(Debug, Serialize)]
pub struct GroupCreatedResponse {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

pub async fn create_group(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateGroupRequest>,
) -> AppResult<Json<ApiResponse<GroupCreatedResponse>>> {
    if req.name.trim().is_empty() || req.name.len() > 80 {
        return Err(AppError::Validation("Group name must be 1-80 chars".into()));
    }
    if req.members.len() < 1 || req.members.len() > 50 {
        return Err(AppError::Validation("Members must be 1-50".into()));
    }
    let me = caller_user_id(&state, &auth).await?;

    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;
    let conv: (Uuid, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO conversations (kind, name, created_by) VALUES ('group', $1, $2)
         RETURNING id, created_at"
    )
    .bind(req.name.trim()).bind(me)
    .fetch_one(&mut *tx).await.map_err(AppError::Database)?;

    // Creator is admin with their own envelope (they computed the
    // group_key locally and sent their own envelope in the request).
    let creator_env = req.members.iter().find(|m| m.user_id == me).map(|m| m.encrypted_group_key.clone());
    sqlx::query(
        "INSERT INTO conversation_members (conversation_id, user_id, role, encrypted_group_key)
         VALUES ($1, $2, 'admin', $3)"
    )
    .bind(conv.0).bind(me).bind(creator_env)
    .execute(&mut *tx).await.map_err(AppError::Database)?;

    for m in &req.members {
        if m.user_id == me { continue; }
        if crate::api::blocks::either_blocks(state.db.pool(), me, m.user_id).await? {
            continue;
        }
        sqlx::query(
            "INSERT INTO group_invitations
                (conversation_id, invited_by, invited_user, encrypted_group_key)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT DO NOTHING"
        )
        .bind(conv.0).bind(me).bind(m.user_id).bind(&m.encrypted_group_key)
        .execute(&mut *tx).await.map_err(AppError::Database)?;
    }

    tx.commit().await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(GroupCreatedResponse {
        id: conv.0, name: req.name.trim().into(), created_at: conv.1,
    })))
}

// ---------- Invite to existing group ----------

#[derive(Debug, Deserialize)]
pub struct InviteRequest {
    pub invited_address: String,
    pub encrypted_group_key: String,
}

pub async fn invite(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(conv_id): Path<Uuid>,
    Json(req): Json<InviteRequest>,
) -> AppResult<Json<ApiResponse<Uuid>>> {
    let me = caller_user_id(&state, &auth).await?;
    let invitee = resolve_user(state.db.pool(), &req.invited_address).await?;
    if invitee == me { return Err(AppError::Validation("Cannot invite yourself".into())); }

    // Only admins of the group can invite.
    let role: Option<String> = sqlx::query_scalar(
        "SELECT cm.role
           FROM conversation_members cm
           JOIN conversations c ON c.id = cm.conversation_id
          WHERE cm.conversation_id = $1 AND cm.user_id = $2 AND c.kind = 'group'"
    )
    .bind(conv_id).bind(me)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    match role.as_deref() {
        Some("admin") => {}
        _ => return Err(AppError::Forbidden("Admins only".into())),
    }
    if crate::api::blocks::either_blocks(state.db.pool(), me, invitee).await? {
        return Err(AppError::Forbidden("Blocked".into()));
    }
    // No duplicate pending invitation.
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM group_invitations
                        WHERE conversation_id = $1 AND invited_user = $2 AND status = 'pending')"
    )
    .bind(conv_id).bind(invitee)
    .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
    if exists { return Err(AppError::Conflict("Already invited".into())); }
    // Already a member?
    let member: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM conversation_members WHERE conversation_id = $1 AND user_id = $2)"
    )
    .bind(conv_id).bind(invitee)
    .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
    if member { return Err(AppError::Conflict("User already a member".into())); }

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO group_invitations (conversation_id, invited_by, invited_user, encrypted_group_key)
         VALUES ($1, $2, $3, $4) RETURNING id"
    )
    .bind(conv_id).bind(me).bind(invitee).bind(&req.encrypted_group_key)
    .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(id)))
}

// ---------- List my invitations ----------

#[derive(Debug, Serialize)]
pub struct InvitationDto {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub conversation_name: Option<String>,
    pub invited_by: Option<Uuid>,
    pub invited_by_username: Option<String>,
    pub encrypted_group_key: String,
    pub created_at: DateTime<Utc>,
}

pub async fn list_mine(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<Vec<InvitationDto>>>> {
    let me = caller_user_id(&state, &auth).await?;
    let rows: Vec<(Uuid, Uuid, Option<String>, Option<Uuid>, Option<String>, String, DateTime<Utc>)> =
        sqlx::query_as(
            "SELECT i.id, i.conversation_id, c.name, i.invited_by, u.username,
                    i.encrypted_group_key, i.created_at
               FROM group_invitations i
               JOIN conversations c ON c.id = i.conversation_id
               LEFT JOIN users u ON u.id = i.invited_by
              WHERE i.invited_user = $1 AND i.status = 'pending'
              ORDER BY i.created_at DESC"
        )
        .bind(me)
        .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;

    let out = rows.into_iter().map(|r| InvitationDto {
        id: r.0, conversation_id: r.1, conversation_name: r.2,
        invited_by: r.3, invited_by_username: r.4,
        encrypted_group_key: r.5, created_at: r.6,
    }).collect();
    Ok(Json(ApiResponse::ok(out)))
}

// ---------- Accept / decline ----------

pub async fn accept(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(inv_id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<Uuid>>> {
    let me = caller_user_id(&state, &auth).await?;
    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;

    let row: Option<(Uuid, String)> = sqlx::query_as(
        "UPDATE group_invitations
            SET status = 'accepted', responded_at = NOW()
          WHERE id = $1 AND invited_user = $2 AND status = 'pending'
          RETURNING conversation_id, encrypted_group_key"
    )
    .bind(inv_id).bind(me)
    .fetch_optional(&mut *tx).await.map_err(AppError::Database)?;

    let (conv_id, env) = row.ok_or_else(|| AppError::NotFound("Invitation not found".into()))?;

    sqlx::query(
        "INSERT INTO conversation_members (conversation_id, user_id, role, encrypted_group_key)
         VALUES ($1, $2, 'member', $3)
         ON CONFLICT DO NOTHING"
    )
    .bind(conv_id).bind(me).bind(&env)
    .execute(&mut *tx).await.map_err(AppError::Database)?;

    tx.commit().await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(conv_id)))
}

pub async fn decline(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(inv_id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    let me = caller_user_id(&state, &auth).await?;
    let n = sqlx::query(
        "UPDATE group_invitations
            SET status = 'declined', responded_at = NOW()
          WHERE id = $1 AND invited_user = $2 AND status = 'pending'"
    )
    .bind(inv_id).bind(me)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;
    if n.rows_affected() == 0 {
        return Err(AppError::NotFound("Invitation not found".into()));
    }
    Ok(Json(ApiResponse::ok("declined")))
}

// ---------- Leave / kick ----------

pub async fn leave(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(conv_id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    let me = caller_user_id(&state, &auth).await?;
    assert_member(state.db.pool(), conv_id, me).await?;

    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;
    sqlx::query("DELETE FROM conversation_members WHERE conversation_id = $1 AND user_id = $2")
        .bind(conv_id).bind(me)
        .execute(&mut *tx).await.map_err(AppError::Database)?;

    // Null out remaining envelopes so the next admin will rotate.
    sqlx::query(
        "UPDATE conversation_members SET encrypted_group_key = NULL
          WHERE conversation_id = $1"
    )
    .bind(conv_id)
    .execute(&mut *tx).await.map_err(AppError::Database)?;

    // If no members left, drop the conversation.
    let remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM conversation_members WHERE conversation_id = $1"
    )
    .bind(conv_id)
    .fetch_one(&mut *tx).await.map_err(AppError::Database)?;
    if remaining == 0 {
        sqlx::query("DELETE FROM conversations WHERE id = $1")
            .bind(conv_id)
            .execute(&mut *tx).await.map_err(AppError::Database)?;
    }

    tx.commit().await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok("left")))
}

pub async fn kick(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((conv_id, target_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    let me = caller_user_id(&state, &auth).await?;
    let role: Option<String> = sqlx::query_scalar(
        "SELECT role FROM conversation_members WHERE conversation_id = $1 AND user_id = $2"
    )
    .bind(conv_id).bind(me)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    if role.as_deref() != Some("admin") {
        return Err(AppError::Forbidden("Admins only".into()));
    }
    if me == target_id {
        return Err(AppError::Validation("Use leave for self".into()));
    }
    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;
    sqlx::query("DELETE FROM conversation_members WHERE conversation_id = $1 AND user_id = $2")
        .bind(conv_id).bind(target_id)
        .execute(&mut *tx).await.map_err(AppError::Database)?;
    // Force rotation: null all envelopes so the next admin will redistribute.
    sqlx::query(
        "UPDATE conversation_members SET encrypted_group_key = NULL
          WHERE conversation_id = $1"
    )
    .bind(conv_id)
    .execute(&mut *tx).await.map_err(AppError::Database)?;
    tx.commit().await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok("kicked")))
}

// ---------- Rotate group key ----------

#[derive(Debug, Deserialize)]
pub struct RotateRequest {
    pub envelopes: Vec<GroupMemberInit>,
}

pub async fn rotate_key(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(conv_id): Path<Uuid>,
    Json(req): Json<RotateRequest>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    let me = caller_user_id(&state, &auth).await?;
    let role: Option<String> = sqlx::query_scalar(
        "SELECT role FROM conversation_members WHERE conversation_id = $1 AND user_id = $2"
    )
    .bind(conv_id).bind(me)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    if role.as_deref() != Some("admin") {
        return Err(AppError::Forbidden("Admins only".into()));
    }
    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;
    for env in &req.envelopes {
        sqlx::query(
            "UPDATE conversation_members SET encrypted_group_key = $3
              WHERE conversation_id = $1 AND user_id = $2"
        )
        .bind(conv_id).bind(env.user_id).bind(&env.encrypted_group_key)
        .execute(&mut *tx).await.map_err(AppError::Database)?;
    }
    tx.commit().await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok("rotated")))
}
