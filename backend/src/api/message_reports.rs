//! Message-level reporting.
//!
//! The crucial invariant the rest of the architecture relies on is
//! that the server is blind to message plaintext. That blocks the
//! conventional path for moderation: an admin can't read a DM to
//! decide whether the reporter is right. We resolve that the only way
//! a legitimate E2EE system can — by handing the disclosure decision
//! to the reporter.
//!
//! Flow:
//! 1. Reporter long-presses a message in their UI and taps "Report".
//! 2. Client locally decrypts the message and prompts: "Share the
//!    decrypted message with our moderation team for this report?"
//!    If they confirm, the decrypted plaintext is submitted in
//!    `disclosed_plaintext`. If they decline, only metadata + reason
//!    is recorded — still useful for abuse-pattern detection (block
//!    rate, report rate, account age) without ever exposing content.
//! 3. The row lives in `message_reports`, never on the messages
//!    table. It's only readable by users with the admin role.
//!
//! Crucially:
//!   * The reporter chooses what gets shared — not the server.
//!   * The disclosed plaintext is per-report, not per-message, so a
//!     single click doesn't blanket-expose a conversation.
//!   * Resolved reports older than 90 days are scrubbed by the
//!     background cleanup (purges `disclosed_plaintext`, keeps the
//!     metadata for trend analysis).

use axum::{extract::{Path, Query, State}, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;
use crate::api::conversations::{assert_member, caller_user_id};
use crate::services::rate_limit;

const ALLOWED_CATEGORIES: &[&str] = &["spam", "abuse", "sexual", "illegal", "other"];
const DISCLOSED_PLAINTEXT_MAX: usize = 4_000;
const REASON_MAX: usize = 500;

#[derive(Debug, Deserialize)]
pub struct ReportMessageRequest {
    /// One of ALLOWED_CATEGORIES. Mirrors post reports so moderators
    /// can triage cross-surface.
    pub category: String,
    /// Free-form reporter context (≤ 500 chars). Stored verbatim, so
    /// the client is responsible for stripping anything they wouldn't
    /// want a moderator to see.
    pub reason: Option<String>,
    /// Opt-in decrypted message content. Leaving this `None` is a
    /// fully valid report — the server still records the metadata so
    /// abuse-pattern signals (sender, block rate, report frequency)
    /// keep working. The presence of this field is a deliberate
    /// disclosure by the reporter and is mirrored back in the
    /// audit trail.
    pub disclosed_plaintext: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ReportSubmitted {
    pub report_id: Uuid,
    pub disclosed: bool,
}

/// POST /api/v1/messages/:id/report
/// Authenticated. Requires the caller to be a member of the
/// conversation that holds the reported message. Rate-limited per
/// reporter to mitigate report-spam.
pub async fn report_message(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(message_id): Path<Uuid>,
    Json(req): Json<ReportMessageRequest>,
) -> AppResult<Json<ApiResponse<ReportSubmitted>>> {
    if !ALLOWED_CATEGORIES.contains(&req.category.as_str()) {
        return Err(AppError::Validation(format!(
            "category must be one of: {}",
            ALLOWED_CATEGORIES.join(", ")
        )));
    }
    if let Some(r) = &req.reason {
        if r.len() > REASON_MAX {
            return Err(AppError::Validation("reason too long".into()));
        }
    }
    if let Some(p) = &req.disclosed_plaintext {
        if p.len() > DISCLOSED_PLAINTEXT_MAX {
            return Err(AppError::Validation("disclosed_plaintext too large".into()));
        }
    }

    let me = caller_user_id(&state, &auth).await?;

    // 10 reports per minute, 60 per hour, per reporter. Generous
    // enough that a power-user clearing a thread won't trip it; tight
    // enough that automated report-bombing of a target gets stopped.
    let principal = me.to_string();
    match rate_limit::check_two_window(
        &state.cache, "msg_report", &principal,
        60, 10,
        3600, 60,
    ).await {
        rate_limit::RateLimitOutcome::Allowed => {}
        _ => return Err(AppError::RateLimited),
    }

    // Resolve the message + verify caller is a conversation member.
    // We deliberately don't reveal "message does not exist" vs
    // "not a member" — both return 404 to avoid an enumeration oracle.
    let row: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(
        "SELECT conversation_id, sender_id FROM messages WHERE id = $1"
    )
    .bind(message_id)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (conv_id, sender_id) = row
        .ok_or_else(|| AppError::NotFound("Message not found".into()))?;
    // assert_member returns Forbidden; map to NotFound here so the
    // existence of the message isn't leaked to non-members.
    if assert_member(state.db.pool(), conv_id, me).await.is_err() {
        return Err(AppError::NotFound("Message not found".into()));
    }

    if sender_id == Some(me) {
        return Err(AppError::Validation("Cannot report your own message".into()));
    }

    let disclosed = req.disclosed_plaintext.is_some();
    let inserted = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO message_reports
            (message_id, conversation_id, reporter_id, reported_user_id,
             category, reason, disclosed_plaintext)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT (message_id, reporter_id) DO NOTHING
         RETURNING id"
    )
    .bind(message_id).bind(conv_id).bind(me).bind(sender_id)
    .bind(&req.category).bind(&req.reason)
    .bind(&req.disclosed_plaintext)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;

    let report_id = match inserted {
        Some(id) => id,
        None => {
            // Duplicate — fetch the existing id to return a stable
            // response. We don't bump priority on duplicate reports.
            sqlx::query_scalar::<_, Uuid>(
                "SELECT id FROM message_reports
                  WHERE message_id = $1 AND reporter_id = $2"
            )
            .bind(message_id).bind(me)
            .fetch_one(state.db.pool()).await.map_err(AppError::Database)?
        }
    };

    Ok(Json(ApiResponse::ok(ReportSubmitted { report_id, disclosed })))
}


// ─── Admin views ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListReportsQuery {
    pub secret: String,
    pub status: Option<String>, // pending | dismissed | actioned | invalid
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ReportRow {
    pub id: Uuid,
    pub message_id: Uuid,
    pub conversation_id: Uuid,
    pub reporter_id: Uuid,
    pub reporter_username: Option<String>,
    pub reported_user_id: Option<Uuid>,
    pub reported_username: Option<String>,
    pub category: String,
    pub reason: Option<String>,
    /// Only populated when the reporter chose to disclose. Admins are
    /// reminded by the UI that this is an explicit, audited disclosure.
    pub disclosed_plaintext: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

/// GET /api/v1/admin/message-reports
pub async fn admin_list_reports(
    State(state): State<AppState>,
    Query(q): Query<ListReportsQuery>,
) -> AppResult<Json<ApiResponse<Vec<ReportRow>>>> {
    crate::api::admin_mod::check_admin_secret(&q.secret)?;

    let status_filter = q.status.clone().unwrap_or_else(|| "pending".into());
    if !["pending", "dismissed", "actioned", "invalid", "all"].contains(&status_filter.as_str()) {
        return Err(AppError::Validation("invalid status filter".into()));
    }

    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let offset = q.offset.unwrap_or(0).max(0);

    let rows: Vec<(
        Uuid, Uuid, Uuid, Uuid, Option<String>,
        Option<Uuid>, Option<String>,
        String, Option<String>, Option<String>,
        String, DateTime<Utc>, Option<DateTime<Utc>>
    )> = if status_filter == "all" {
        sqlx::query_as(
            "SELECT r.id, r.message_id, r.conversation_id,
                    r.reporter_id, ru.username AS reporter_username,
                    r.reported_user_id, su.username AS reported_username,
                    r.category, r.reason, r.disclosed_plaintext,
                    r.status, r.created_at, r.resolved_at
               FROM message_reports r
               LEFT JOIN users ru ON ru.id = r.reporter_id
               LEFT JOIN users su ON su.id = r.reported_user_id
              ORDER BY r.created_at DESC
              LIMIT $1 OFFSET $2"
        )
        .bind(limit).bind(offset)
        .fetch_all(state.db.pool()).await.map_err(AppError::Database)?
    } else {
        sqlx::query_as(
            "SELECT r.id, r.message_id, r.conversation_id,
                    r.reporter_id, ru.username AS reporter_username,
                    r.reported_user_id, su.username AS reported_username,
                    r.category, r.reason, r.disclosed_plaintext,
                    r.status, r.created_at, r.resolved_at
               FROM message_reports r
               LEFT JOIN users ru ON ru.id = r.reporter_id
               LEFT JOIN users su ON su.id = r.reported_user_id
              WHERE r.status = $3
              ORDER BY r.created_at DESC
              LIMIT $1 OFFSET $2"
        )
        .bind(limit).bind(offset).bind(&status_filter)
        .fetch_all(state.db.pool()).await.map_err(AppError::Database)?
    };

    let out = rows.into_iter().map(|r| ReportRow {
        id: r.0, message_id: r.1, conversation_id: r.2,
        reporter_id: r.3, reporter_username: r.4,
        reported_user_id: r.5, reported_username: r.6,
        category: r.7, reason: r.8, disclosed_plaintext: r.9,
        status: r.10, created_at: r.11, resolved_at: r.12,
    }).collect();
    Ok(Json(ApiResponse::ok(out)))
}

#[derive(Debug, Deserialize)]
pub struct ResolveReportRequest {
    pub secret: String,
    /// One of 'dismissed' | 'actioned' | 'invalid'. 'pending' is
    /// invalid here (would un-resolve).
    pub resolution: String,
    pub note: Option<String>,
}

/// POST /api/v1/admin/message-reports/:id/resolve
pub async fn admin_resolve_report(
    State(state): State<AppState>,
    Path(report_id): Path<Uuid>,
    Json(req): Json<ResolveReportRequest>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    crate::api::admin_mod::check_admin_secret(&req.secret)?;
    if !["dismissed", "actioned", "invalid"].contains(&req.resolution.as_str()) {
        return Err(AppError::Validation("resolution must be dismissed|actioned|invalid".into()));
    }
    if let Some(n) = &req.note {
        if n.len() > 1_000 {
            return Err(AppError::Validation("note too long".into()));
        }
    }

    let updated = sqlx::query(
        "UPDATE message_reports
            SET status = $2, resolution = $3, resolved_at = NOW()
          WHERE id = $1 AND status = 'pending'"
    )
    .bind(report_id).bind(&req.resolution).bind(&req.note)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;

    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound("Pending report not found".into()));
    }
    Ok(Json(ApiResponse::ok("resolved")))
}
