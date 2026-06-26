//! Real 18+ verification with manual admin review.
//!
//! Replaces the self-declared age_verified_at flag from migration 0021
//! (which was set client-side via a self-confirmed face scan) with a
//! queue: user submits face scan + optional ID document → encrypted
//! blobs stored under PRIVATE_DIR → case marked pending → admin
//! approves/rejects → on approval, `users.age_verified_at` is set and
//! the purple badge becomes visible.
//!
//! Privacy posture in one paragraph: biometric face data and
//! government IDs live ONLY in the per-case encrypted blobs (see
//! services::pii_vault) and are scrubbed shortly after the decision
//! (7d approved, 30d rejected). Admin access goes through both the
//! shared admin secret AND the admin's JWT (logged in admin_actions),
//! so every PII fetch is attributable.

use axum::{
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::{AuthUser, OptionalAuth};
use crate::api::admin_mod::check_admin_secret;
use crate::services::pii_vault;

// 8 MB cap per blob (face JPEG/PNG/WebP, ID photo same shape).
const BLOB_MAX_BYTES: usize = 8 * 1024 * 1024;
const NOTE_MAX_LEN: usize = 1_000;

async fn caller_user_id(state: &AppState, auth: &AuthUser) -> AppResult<Uuid> {
    if let Some(rest) = auth.address.strip_prefix("email:") {
        return Uuid::parse_str(rest).map_err(|_| AppError::Validation("Invalid user id".into()));
    }
    sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
        .bind(&auth.address)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))
}

fn allowed_id_type(s: &str) -> bool {
    matches!(s, "passport" | "driver_license" | "national_id" | "other")
}

fn is_image_ct(ct: &str) -> bool {
    matches!(ct, "image/jpeg" | "image/jpg" | "image/png" | "image/webp")
}

// ── User-facing endpoints ────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct VerificationStatus {
    /// 'pending' | 'approved' | 'rejected' | 'withdrawn' | 'none'
    pub status: String,
    pub submitted_at: Option<DateTime<Utc>>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub decision_note: Option<String>,
    /// True iff users.age_verified_at is set. May be true with no
    /// case visible here (carry-over from the old self-declared flow).
    pub age_verified: bool,
    pub badge_hidden: bool,
    /// False if AGE_VERIFY_KEY isn't configured; submit endpoint will
    /// refuse until the operator sets it.
    pub server_configured: bool,
}

/// GET /api/v1/me/age-verification/status
pub async fn get_status(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<VerificationStatus>>> {
    let me = caller_user_id(&state, &auth).await?;
    let user_row: Option<(Option<DateTime<Utc>>, bool)> = sqlx::query_as(
        "SELECT age_verified_at, age_badge_hidden FROM users WHERE id = $1"
    )
    .bind(me)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (age_verified_at, badge_hidden) = user_row.unwrap_or((None, false));

    // Newest case (any status) gets surfaced — withdrawn shows up too
    // so the UI can offer "submit again".
    let case: Option<(String, DateTime<Utc>, Option<DateTime<Utc>>, Option<String>)> =
        sqlx::query_as(
            "SELECT status, submitted_at, reviewed_at, decision_note
               FROM age_verification_cases
              WHERE user_id = $1
              ORDER BY submitted_at DESC
              LIMIT 1"
        )
        .bind(me)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;

    let (status, submitted_at, reviewed_at, decision_note) = match case {
        Some((s, sa, ra, n)) => (s, Some(sa), ra, n),
        None => ("none".to_string(), None, None, None),
    };

    Ok(Json(ApiResponse::ok(VerificationStatus {
        status,
        submitted_at,
        reviewed_at,
        decision_note,
        age_verified: age_verified_at.is_some(),
        badge_hidden,
        server_configured: pii_vault::is_configured(),
    })))
}

/// POST /api/v1/me/age-verification/submit
/// Multipart: face_scan (required image), id_document (optional image),
///            id_type (optional: passport|driver_license|national_id|other).
pub async fn submit(
    State(state): State<AppState>,
    auth: AuthUser,
    mut mp: Multipart,
) -> AppResult<Json<ApiResponse<Uuid>>> {
    if !pii_vault::is_configured() {
        return Err(AppError::Internal(
            "Age verification is not configured on this server".into()
        ));
    }
    let me = caller_user_id(&state, &auth).await?;

    let mut face_bytes: Option<Vec<u8>> = None;
    let mut id_bytes: Option<Vec<u8>> = None;
    let mut id_type: Option<String> = None;

    while let Some(field) = mp.next_field().await
        .map_err(|e| AppError::Validation(format!("Multipart parse error: {e}")))?
    {
        let name = field.name().map(|s| s.to_string()).unwrap_or_default();
        let ct = field.content_type().map(|s| s.to_string()).unwrap_or_default();
        match name.as_str() {
            "face_scan" => {
                if !is_image_ct(&ct) {
                    return Err(AppError::Validation("face_scan must be JPEG/PNG/WebP".into()));
                }
                let b = field.bytes().await
                    .map_err(|e| AppError::Validation(format!("face read: {e}")))?;
                if b.is_empty() || b.len() > BLOB_MAX_BYTES {
                    return Err(AppError::Validation("face_scan size out of range".into()));
                }
                face_bytes = Some(b.to_vec());
            }
            "id_document" => {
                if !is_image_ct(&ct) {
                    return Err(AppError::Validation("id_document must be JPEG/PNG/WebP".into()));
                }
                let b = field.bytes().await
                    .map_err(|e| AppError::Validation(format!("id read: {e}")))?;
                if b.is_empty() || b.len() > BLOB_MAX_BYTES {
                    return Err(AppError::Validation("id_document size out of range".into()));
                }
                id_bytes = Some(b.to_vec());
            }
            "id_type" => {
                let txt = field.text().await
                    .map_err(|e| AppError::Validation(format!("id_type read: {e}")))?;
                let t = txt.trim().to_string();
                if !allowed_id_type(&t) {
                    return Err(AppError::Validation("invalid id_type".into()));
                }
                id_type = Some(t);
            }
            _ => {}
        }
    }

    let face = face_bytes.ok_or_else(|| AppError::Validation("face_scan field required".into()))?;
    if id_bytes.is_some() && id_type.is_none() {
        return Err(AppError::Validation("id_type required when id_document is provided".into()));
    }

    // Withdraw any previous pending case (also nuke its blobs) so the
    // UNIQUE partial index doesn't trip and old blobs don't linger.
    let prev: Vec<(Uuid, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT id, face_scan_path, id_document_path FROM age_verification_cases
          WHERE user_id = $1 AND status = 'pending'"
    )
    .bind(me)
    .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;
    for (cid, fp, ip) in prev {
        sqlx::query(
            "UPDATE age_verification_cases
                SET status = 'withdrawn',
                    blobs_purged_at = NOW(),
                    face_scan_path = NULL,
                    id_document_path = NULL
              WHERE id = $1"
        )
        .bind(cid)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;
        if let Some(p) = fp { let _ = pii_vault::purge_blob(&p).await; }
        if let Some(p) = ip { let _ = pii_vault::purge_blob(&p).await; }
    }

    let case_id: Uuid = sqlx::query_scalar(
        "INSERT INTO age_verification_cases
            (user_id, status, id_document_type)
         VALUES ($1, 'pending', $2)
         RETURNING id"
    )
    .bind(me).bind(&id_type)
    .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;

    // Encrypt + persist the blobs.
    let face_path = pii_vault::write_blob(case_id, "face", &face).await
        .map_err(AppError::Internal)?;
    sqlx::query("UPDATE age_verification_cases SET face_scan_path = $2 WHERE id = $1")
        .bind(case_id).bind(&face_path)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;
    if let Some(id_blob) = id_bytes {
        let id_path = pii_vault::write_blob(case_id, "id", &id_blob).await
            .map_err(AppError::Internal)?;
        sqlx::query("UPDATE age_verification_cases SET id_document_path = $2 WHERE id = $1")
            .bind(case_id).bind(&id_path)
            .execute(state.db.pool()).await.map_err(AppError::Database)?;
    }

    Ok(Json(ApiResponse::ok(case_id)))
}

/// POST /api/v1/me/age-verification/withdraw
/// Withdraws this user's pending case (if any) and nukes its blobs.
pub async fn withdraw(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    let me = caller_user_id(&state, &auth).await?;
    let row: Option<(Uuid, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT id, face_scan_path, id_document_path FROM age_verification_cases
          WHERE user_id = $1 AND status = 'pending'"
    )
    .bind(me)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let Some((case_id, fp, ip)) = row else {
        return Err(AppError::NotFound("no pending case".into()));
    };
    sqlx::query(
        "UPDATE age_verification_cases
            SET status = 'withdrawn', blobs_purged_at = NOW(),
                face_scan_path = NULL, id_document_path = NULL
          WHERE id = $1"
    )
    .bind(case_id)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;
    if let Some(p) = fp { let _ = pii_vault::purge_blob(&p).await; }
    if let Some(p) = ip { let _ = pii_vault::purge_blob(&p).await; }
    Ok(Json(ApiResponse::ok("withdrawn")))
}

#[derive(Debug, Deserialize)]
pub struct BadgePref { pub hidden: bool }

/// PATCH /api/v1/me/age-badge — toggle badge visibility (does NOT
/// revoke the verification itself; the user can flip it back on).
pub async fn set_badge_visibility(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<BadgePref>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    let me = caller_user_id(&state, &auth).await?;
    sqlx::query("UPDATE users SET age_badge_hidden = $2 WHERE id = $1")
        .bind(me).bind(req.hidden)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok("ok")))
}


// ── Admin endpoints ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AdminListQuery {
    pub secret: String,
    /// 'pending' (default) | 'approved' | 'rejected' | 'withdrawn' | 'all'
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct AdminCaseRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub username: Option<String>,
    pub status: String,
    pub id_document_type: Option<String>,
    pub has_face_blob: bool,
    pub has_id_blob: bool,
    pub submitted_at: DateTime<Utc>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub decision_note: Option<String>,
    pub blobs_purged_at: Option<DateTime<Utc>>,
}

/// GET /api/v1/admin/age-verification?secret=...
pub async fn admin_list(
    State(state): State<AppState>,
    Query(q): Query<AdminListQuery>,
) -> AppResult<Json<ApiResponse<Vec<AdminCaseRow>>>> {
    check_admin_secret(&q.secret)?;
    let status_filter = q.status.unwrap_or_else(|| "pending".into());
    if !["pending", "approved", "rejected", "withdrawn", "all"].contains(&status_filter.as_str()) {
        return Err(AppError::Validation("invalid status filter".into()));
    }
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let offset = q.offset.unwrap_or(0).max(0);

    let rows: Vec<(Uuid, Uuid, Option<String>, String, Option<String>, Option<String>, Option<String>,
                   DateTime<Utc>, Option<DateTime<Utc>>, Option<String>, Option<DateTime<Utc>>)> =
        if status_filter == "all" {
            sqlx::query_as(
                "SELECT c.id, c.user_id, u.username, c.status,
                        c.id_document_type, c.face_scan_path, c.id_document_path,
                        c.submitted_at, c.reviewed_at, c.decision_note, c.blobs_purged_at
                   FROM age_verification_cases c
                   LEFT JOIN users u ON u.id = c.user_id
                  ORDER BY c.submitted_at DESC
                  LIMIT $1 OFFSET $2"
            )
            .bind(limit).bind(offset)
            .fetch_all(state.db.pool()).await.map_err(AppError::Database)?
        } else {
            sqlx::query_as(
                "SELECT c.id, c.user_id, u.username, c.status,
                        c.id_document_type, c.face_scan_path, c.id_document_path,
                        c.submitted_at, c.reviewed_at, c.decision_note, c.blobs_purged_at
                   FROM age_verification_cases c
                   LEFT JOIN users u ON u.id = c.user_id
                  WHERE c.status = $3
                  ORDER BY c.submitted_at DESC
                  LIMIT $1 OFFSET $2"
            )
            .bind(limit).bind(offset).bind(&status_filter)
            .fetch_all(state.db.pool()).await.map_err(AppError::Database)?
        };

    let out = rows.into_iter().map(|r| AdminCaseRow {
        id: r.0, user_id: r.1, username: r.2, status: r.3,
        id_document_type: r.4,
        has_face_blob: r.5.is_some(),
        has_id_blob: r.6.is_some(),
        submitted_at: r.7, reviewed_at: r.8, decision_note: r.9, blobs_purged_at: r.10,
    }).collect();
    Ok(Json(ApiResponse::ok(out)))
}

#[derive(Debug, Deserialize)]
pub struct AdminBlobQuery {
    pub secret: String,
    /// 'face' | 'id'
    pub slot: String,
}

/// GET /api/v1/admin/age-verification/:case_id/blob?secret=&slot=
/// Returns the decrypted image bytes. Sets strict no-store headers.
pub async fn admin_get_blob(
    State(state): State<AppState>,
    Path(case_id): Path<Uuid>,
    Query(q): Query<AdminBlobQuery>,
) -> Result<Response, AppError> {
    check_admin_secret(&q.secret)?;
    if !matches!(q.slot.as_str(), "face" | "id") {
        return Err(AppError::Validation("slot must be 'face' or 'id'".into()));
    }

    let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT face_scan_path, id_document_path
           FROM age_verification_cases WHERE id = $1"
    )
    .bind(case_id)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (face, idp) = row.ok_or_else(|| AppError::NotFound("case not found".into()))?;
    let rel = match q.slot.as_str() {
        "face" => face,
        "id"   => idp,
        _ => None,
    };
    let rel = rel.ok_or_else(|| AppError::NotFound("blob purged or not present".into()))?;
    let bytes = pii_vault::read_blob(case_id, &q.slot, &rel).await
        .map_err(|_| AppError::NotFound("blob unreadable".into()))?;

    let mut headers = HeaderMap::new();
    // We accepted JPEG/PNG/WebP — the browser sniffs the magic bytes.
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    headers.insert("X-Content-Type-Options", HeaderValue::from_static("nosniff"));
    Ok((StatusCode::OK, headers, Body::from(bytes)).into_response())
}

#[derive(Debug, Deserialize)]
pub struct AdminDecideRequest {
    pub secret: String,
    pub note: Option<String>,
}

/// POST /api/v1/admin/age-verification/:case_id/approve
/// Sets users.age_verified_at, schedules blobs for cleanup (via the
/// existing message-cleanup job's prune pass), records an audit entry.
pub async fn admin_approve(
    State(state): State<AppState>,
    viewer: OptionalAuth,
    Path(case_id): Path<Uuid>,
    Json(req): Json<AdminDecideRequest>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    check_admin_secret(&req.secret)?;
    if let Some(n) = &req.note { if n.len() > NOTE_MAX_LEN { return Err(AppError::Validation("note too long".into())); } }

    let case: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT user_id, status FROM age_verification_cases WHERE id = $1"
    )
    .bind(case_id)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (user_id, status) = case.ok_or_else(|| AppError::NotFound("case not found".into()))?;
    if status != "pending" {
        return Err(AppError::Validation("case is not pending".into()));
    }

    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;
    sqlx::query(
        "UPDATE age_verification_cases
            SET status = 'approved', reviewed_at = NOW(),
                decision_note = $2
          WHERE id = $1"
    )
    .bind(case_id).bind(&req.note)
    .execute(&mut *tx).await.map_err(AppError::Database)?;
    sqlx::query(
        "UPDATE users SET age_verified_at = COALESCE(age_verified_at, NOW()) WHERE id = $1"
    )
    .bind(user_id)
    .execute(&mut *tx).await.map_err(AppError::Database)?;
    tx.commit().await.map_err(AppError::Database)?;

    let target_username: Option<String> = sqlx::query_scalar(
        "SELECT username FROM users WHERE id = $1"
    ).bind(user_id).fetch_optional(state.db.pool()).await.ok().flatten().flatten();
    let (admin_id, admin_name) = resolve_admin_actor(&state, &viewer).await;
    crate::api::admin_mod::record_action(
        state.db.pool(), Some(user_id), target_username.as_deref(),
        "age_verify_approve", None, req.note.as_deref(),
        admin_id, admin_name.as_deref(),
    ).await;

    Ok(Json(ApiResponse::ok("approved")))
}

/// POST /api/v1/admin/age-verification/:case_id/reject
pub async fn admin_reject(
    State(state): State<AppState>,
    viewer: OptionalAuth,
    Path(case_id): Path<Uuid>,
    Json(req): Json<AdminDecideRequest>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    check_admin_secret(&req.secret)?;
    if let Some(n) = &req.note { if n.len() > NOTE_MAX_LEN { return Err(AppError::Validation("note too long".into())); } }
    let case: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT user_id, status FROM age_verification_cases WHERE id = $1"
    )
    .bind(case_id)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (user_id, status) = case.ok_or_else(|| AppError::NotFound("case not found".into()))?;
    if status != "pending" {
        return Err(AppError::Validation("case is not pending".into()));
    }
    sqlx::query(
        "UPDATE age_verification_cases
            SET status = 'rejected', reviewed_at = NOW(),
                decision_note = $2
          WHERE id = $1"
    )
    .bind(case_id).bind(&req.note)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;

    let target_username: Option<String> = sqlx::query_scalar(
        "SELECT username FROM users WHERE id = $1"
    ).bind(user_id).fetch_optional(state.db.pool()).await.ok().flatten().flatten();
    let (admin_id, admin_name) = resolve_admin_actor(&state, &viewer).await;
    crate::api::admin_mod::record_action(
        state.db.pool(), Some(user_id), target_username.as_deref(),
        "age_verify_reject", None, req.note.as_deref(),
        admin_id, admin_name.as_deref(),
    ).await;
    Ok(Json(ApiResponse::ok("rejected")))
}

/// POST /api/v1/admin/users/:address/age-verify/revoke
/// Strips a user's age_verified_at (e.g. retraction after the fact).
/// Audit-logged.
#[derive(Debug, Deserialize)]
pub struct AdminRevokeRequest {
    pub secret: String,
    pub reason: Option<String>,
}

pub async fn admin_revoke_verification(
    State(state): State<AppState>,
    viewer: OptionalAuth,
    Path(address): Path<String>,
    Json(req): Json<AdminRevokeRequest>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    check_admin_secret(&req.secret)?;
    if let Some(r) = &req.reason { if r.len() > NOTE_MAX_LEN { return Err(AppError::Validation("reason too long".into())); } }

    let user_id = crate::api::conversations::resolve_user(state.db.pool(), &address).await?;
    sqlx::query("UPDATE users SET age_verified_at = NULL WHERE id = $1")
        .bind(user_id)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;
    let target_username: Option<String> = sqlx::query_scalar(
        "SELECT username FROM users WHERE id = $1"
    ).bind(user_id).fetch_optional(state.db.pool()).await.ok().flatten().flatten();
    let (admin_id, admin_name) = resolve_admin_actor(&state, &viewer).await;
    crate::api::admin_mod::record_action(
        state.db.pool(), Some(user_id), target_username.as_deref(),
        "age_verify_revoke", None, req.reason.as_deref(),
        admin_id, admin_name.as_deref(),
    ).await;
    Ok(Json(ApiResponse::ok("revoked")))
}

// Identical to admin_mod::admin_actor; kept private to avoid leaking
// the helper signature into the public API.
async fn resolve_admin_actor(state: &AppState, viewer: &OptionalAuth) -> (Option<Uuid>, Option<String>) {
    let auth = match &viewer.0 { Some(a) => a, None => return (None, None) };
    let id_opt: Option<Uuid> = if let Some(rest) = auth.address.strip_prefix("email:") {
        Uuid::parse_str(rest).ok()
    } else {
        sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
            .bind(&auth.address)
            .fetch_optional(state.db.pool()).await.ok().flatten()
    };
    let Some(id) = id_opt else { return (None, None); };
    let username: Option<String> = sqlx::query_scalar(
        "SELECT username FROM users WHERE id = $1"
    ).bind(id).fetch_optional(state.db.pool()).await.ok().flatten();
    (Some(id), username)
}
