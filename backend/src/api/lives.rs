//! Live-stream lifecycle: create / schedule, list active + upcoming,
//! start/end/cancel, and tip-driven ranking.
//!
//! Phase 1 ships the data + signalling story end-to-end without a real
//! media pipe. A `lives` row carries the host, schedule slot, viewer
//! count and tip total; `lives.tip_total_yeet` is materialised on read
//! by summing the existing `tips.live_id` linkage, which keeps the
//! ranking score self-consistent with whatever `send_tip_tx` did.
//!
//! Phase 2 will populate `livekit_room` and add a WebRTC signalling
//! WebSocket; everything in this file is forward-compatible with that
//! — the API surface (`POST /lives/:id/start`) is where we'll mint the
//! LiveKit token and return it to the host client.
use axum::{extract::{Path, Query, State}, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;

/// Resolve the calling user id from either a wallet address or the
/// `email:UUID` synthetic address used by the email/password flow.
async fn resolve_user_id(state: &AppState, auth_address: &str) -> AppResult<Uuid> {
    if let Some(uuid_str) = auth_address.strip_prefix("email:") {
        return uuid_str.parse::<Uuid>()
            .map_err(|_| AppError::NotFound("Invalid user ID".into()));
    }
    sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
        .bind(auth_address)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))
}

// ─── DTOs ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateLiveRequest {
    pub title: String,
    pub description: Option<String>,
    pub scheduled_for: Option<DateTime<Utc>>, // None = go live now
    pub is_adult: Option<bool>,
    pub cover_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LiveSummary {
    pub id: Uuid,
    pub host: LiveHost,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub scheduled_for: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub viewer_count: i32,
    pub tip_total_yeet: f64,
    pub is_adult: bool,
    pub cover_url: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct LiveHost {
    pub id: Uuid,
    pub wallet_address: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(sqlx::FromRow)]
struct LiveRow {
    id: Uuid,
    host_user_id: Uuid,
    title: String,
    description: Option<String>,
    status: String,
    scheduled_for: Option<DateTime<Utc>>,
    started_at: Option<DateTime<Utc>>,
    ended_at: Option<DateTime<Utc>>,
    viewer_count: i32,
    is_adult: bool,
    cover_url: Option<String>,
    created_at: DateTime<Utc>,
    wallet_address: Option<String>,
    display_name: Option<String>,
    avatar_url: Option<String>,
    tip_total_yeet: f64,
}

fn row_to_summary(r: LiveRow) -> LiveSummary {
    LiveSummary {
        id: r.id,
        host: LiveHost {
            id: r.host_user_id,
            wallet_address: r.wallet_address,
            display_name: r.display_name,
            avatar_url: r.avatar_url,
        },
        title: r.title,
        description: r.description,
        status: r.status,
        scheduled_for: r.scheduled_for,
        started_at: r.started_at,
        ended_at: r.ended_at,
        viewer_count: r.viewer_count,
        tip_total_yeet: r.tip_total_yeet,
        is_adult: r.is_adult,
        cover_url: r.cover_url,
        created_at: r.created_at,
    }
}

// ─── Handlers ─────────────────────────────────────────────────────────────

pub async fn create_live(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateLiveRequest>,
) -> AppResult<Json<ApiResponse<Uuid>>> {
    let title = req.title.trim();
    if title.is_empty() || title.len() > 120 {
        return Err(AppError::Validation("Title must be 1-120 chars".into()));
    }
    if let Some(d) = &req.description {
        if d.len() > 500 {
            return Err(AppError::Validation("Description must be ≤ 500 chars".into()));
        }
    }
    // Reject schedules in the past. Allow a 60-second forgiveness window
    // to absorb clock skew between client and server.
    if let Some(t) = req.scheduled_for {
        if t < Utc::now() - chrono::Duration::seconds(60) {
            return Err(AppError::Validation("Scheduled time must be in the future".into()));
        }
        if t > Utc::now() + chrono::Duration::days(60) {
            return Err(AppError::Validation("Cannot schedule more than 60 days ahead".into()));
        }
    }
    let user_id = resolve_user_id(&state, &auth.address).await?;

    // "Go live now" path: no scheduled_for, status starts as 'live'.
    // We don't mint the LiveKit room here yet; that happens in Phase 2.
    let now_live = req.scheduled_for.is_none();
    let status = if now_live { "live" } else { "scheduled" };
    let started_at = if now_live { Some(Utc::now()) } else { None };

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO lives
           (host_user_id, title, description, status, scheduled_for,
            started_at, is_adult, cover_url)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING id"
    )
    .bind(user_id).bind(title).bind(&req.description)
    .bind(status).bind(req.scheduled_for).bind(started_at)
    .bind(req.is_adult.unwrap_or(false))
    .bind(&req.cover_url)
    .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(id)))
}

#[derive(Debug, Deserialize)]
pub struct ListLivesQuery {
    pub include_adult: Option<bool>,
    pub limit: Option<i64>,
}

/// Active broadcasts sorted by tip-total DESC, then viewer_count DESC,
/// then started_at ASC. That ordering is the "tip pushes you up the
/// list" rule the product asked for.
pub async fn list_active(
    State(state): State<AppState>,
    Query(q): Query<ListLivesQuery>,
) -> AppResult<Json<ApiResponse<Vec<LiveSummary>>>> {
    let include_adult = q.include_adult.unwrap_or(false);
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let rows = sqlx::query_as::<_, LiveRow>(
        "SELECT l.id, l.host_user_id, l.title, l.description, l.status,
                l.scheduled_for, l.started_at, l.ended_at,
                l.viewer_count, l.is_adult, l.cover_url, l.created_at,
                u.wallet_address, u.display_name, u.avatar_url,
                COALESCE((SELECT SUM(amount::float8) FROM tips t
                          WHERE t.live_id = l.id AND t.currency = 'YEET'), 0.0)
                  AS tip_total_yeet
           FROM lives l
           JOIN users u ON u.id = l.host_user_id
          WHERE l.status = 'live'
            AND ($1 OR l.is_adult = FALSE)
          ORDER BY
              COALESCE((SELECT SUM(amount::float8) FROM tips t
                        WHERE t.live_id = l.id AND t.currency = 'YEET'), 0.0) DESC,
              l.viewer_count DESC,
              l.started_at ASC
          LIMIT $2"
    )
    .bind(include_adult)
    .bind(limit)
    .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(rows.into_iter().map(row_to_summary).collect())))
}

/// Upcoming scheduled lives in the next 30 days, ordered by start time.
pub async fn list_scheduled(
    State(state): State<AppState>,
    Query(q): Query<ListLivesQuery>,
) -> AppResult<Json<ApiResponse<Vec<LiveSummary>>>> {
    let include_adult = q.include_adult.unwrap_or(false);
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let rows = sqlx::query_as::<_, LiveRow>(
        "SELECT l.id, l.host_user_id, l.title, l.description, l.status,
                l.scheduled_for, l.started_at, l.ended_at,
                l.viewer_count, l.is_adult, l.cover_url, l.created_at,
                u.wallet_address, u.display_name, u.avatar_url,
                0.0::float8 AS tip_total_yeet
           FROM lives l
           JOIN users u ON u.id = l.host_user_id
          WHERE l.status = 'scheduled'
            AND ($1 OR l.is_adult = FALSE)
            AND (l.scheduled_for IS NULL OR l.scheduled_for > NOW())
            AND (l.scheduled_for IS NULL OR l.scheduled_for < NOW() + INTERVAL '30 days')
          ORDER BY COALESCE(l.scheduled_for, l.created_at) ASC
          LIMIT $2"
    )
    .bind(include_adult)
    .bind(limit)
    .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(rows.into_iter().map(row_to_summary).collect())))
}

pub async fn get_live(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<LiveSummary>>> {
    let row = sqlx::query_as::<_, LiveRow>(
        "SELECT l.id, l.host_user_id, l.title, l.description, l.status,
                l.scheduled_for, l.started_at, l.ended_at,
                l.viewer_count, l.is_adult, l.cover_url, l.created_at,
                u.wallet_address, u.display_name, u.avatar_url,
                COALESCE((SELECT SUM(amount::float8) FROM tips t
                          WHERE t.live_id = l.id AND t.currency = 'YEET'), 0.0)
                  AS tip_total_yeet
           FROM lives l
           JOIN users u ON u.id = l.host_user_id
          WHERE l.id = $1"
    )
    .bind(id)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
    .ok_or_else(|| AppError::NotFound("Live not found".into()))?;

    Ok(Json(ApiResponse::ok(row_to_summary(row))))
}

/// Host transitions a scheduled broadcast to 'live'. Idempotent: if
/// the live is already live, returns OK.
pub async fn start_live(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<()>>> {
    let user_id = resolve_user_id(&state, &auth.address).await?;
    // Verify the caller is the host and the live is in a startable state.
    let row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT host_user_id, status FROM lives WHERE id = $1"
    ).bind(id).fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (host_id, status) = row.ok_or_else(|| AppError::NotFound("Live not found".into()))?;
    if host_id != user_id {
        return Err(AppError::Forbidden("Only the host can start this live".into()));
    }
    match status.as_str() {
        "live" => return Ok(Json(ApiResponse::ok(()))),
        "ended" | "cancelled" => return Err(AppError::Validation("Live already finished".into())),
        _ => {}
    }
    // Phase 2 will mint a LiveKit room name + token here.
    sqlx::query(
        "UPDATE lives
            SET status = 'live',
                started_at = COALESCE(started_at, NOW())
          WHERE id = $1"
    ).bind(id).execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(())))
}

pub async fn end_live(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<()>>> {
    let user_id = resolve_user_id(&state, &auth.address).await?;
    let row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT host_user_id, status FROM lives WHERE id = $1"
    ).bind(id).fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (host_id, status) = row.ok_or_else(|| AppError::NotFound("Live not found".into()))?;
    if host_id != user_id {
        return Err(AppError::Forbidden("Only the host can end this live".into()));
    }
    if status == "ended" || status == "cancelled" {
        return Ok(Json(ApiResponse::ok(())));
    }
    sqlx::query(
        "UPDATE lives SET status = 'ended', ended_at = NOW() WHERE id = $1"
    ).bind(id).execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(())))
}

/// Cancel a scheduled (not-yet-started) live.
pub async fn cancel_live(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<()>>> {
    let user_id = resolve_user_id(&state, &auth.address).await?;
    let row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT host_user_id, status FROM lives WHERE id = $1"
    ).bind(id).fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (host_id, status) = row.ok_or_else(|| AppError::NotFound("Live not found".into()))?;
    if host_id != user_id {
        return Err(AppError::Forbidden("Only the host can cancel this live".into()));
    }
    if status != "scheduled" {
        return Err(AppError::Validation("Only scheduled lives can be cancelled".into()));
    }
    sqlx::query("UPDATE lives SET status = 'cancelled' WHERE id = $1")
        .bind(id).execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(())))
}

// ─── Viewer count + tipping ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct JoinLeaveRequest {
    pub delta: Option<i32>, // +1 on join, -1 on leave; default +1
}

/// Best-effort viewer counter. Real WebRTC viewer tracking will replace
/// this in Phase 2 — for now, the client posts +1 on join and -1 on
/// leave. Floors at zero.
pub async fn ping_viewer_count(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<JoinLeaveRequest>,
) -> AppResult<Json<ApiResponse<i32>>> {
    let delta = req.delta.unwrap_or(1).clamp(-1, 1);
    let updated: Option<i32> = sqlx::query_scalar(
        "UPDATE lives
            SET viewer_count = GREATEST(0, viewer_count + $1)
          WHERE id = $2 AND status = 'live'
          RETURNING viewer_count"
    )
    .bind(delta).bind(id)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(updated.unwrap_or(0))))
}

#[derive(Debug, Deserialize)]
pub struct TipLiveRequest {
    pub amount: String,           // YEET tokens as decimal string
    pub tx_hash: Option<String>,  // on-chain reference, optional
}

#[derive(Debug, Serialize)]
pub struct TipLiveResponse {
    pub tip_id: Uuid,
    pub tip_total_yeet: f64,
}

/// Send a YEET tip directly to a live's host. Uses the same
/// `send_tip_tx` plumbing as post tips so balance accounting and
/// platform-fee handling stay in one place. Attaches `live_id` so the
/// ranking query sees the new total immediately.
pub async fn tip_live(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<TipLiveRequest>,
) -> AppResult<Json<ApiResponse<TipLiveResponse>>> {
    let from_id = resolve_user_id(&state, &auth.address).await?;

    // Look up the host so we can route the tip directly to them.
    let host_id: Uuid = sqlx::query_scalar(
        "SELECT host_user_id FROM lives WHERE id = $1 AND status = 'live'"
    )
    .bind(id)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
    .ok_or_else(|| AppError::NotFound("Live not currently broadcasting".into()))?;

    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;
    let tip_id = crate::api::tips::send_tip_tx(
        &mut tx, from_id, host_id, None, &req.amount, "YEET", req.tx_hash.as_deref()
    ).await?;
    // Attach this tip to the live so the ranking query sees it. We can't
    // pass live_id into `send_tip_tx` without changing its signature, so
    // we patch it on the same row in the same tx.
    sqlx::query("UPDATE tips SET live_id = $1 WHERE id = $2")
        .bind(id).bind(tip_id)
        .execute(&mut *tx).await.map_err(AppError::Database)?;
    tx.commit().await.map_err(AppError::Database)?;

    // Read back the fresh total so the client can update its ranking
    // without re-listing.
    let total: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount::float8), 0.0)
           FROM tips WHERE live_id = $1 AND currency = 'YEET'"
    ).bind(id).fetch_one(state.db.pool()).await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(TipLiveResponse { tip_id, tip_total_yeet: total })))
}

/// List the calling user's own scheduled + live + recent ended lives,
/// so the profile view can show "Geplante Lives".
pub async fn list_mine(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<Vec<LiveSummary>>>> {
    let user_id = resolve_user_id(&state, &auth.address).await?;
    let rows = sqlx::query_as::<_, LiveRow>(
        "SELECT l.id, l.host_user_id, l.title, l.description, l.status,
                l.scheduled_for, l.started_at, l.ended_at,
                l.viewer_count, l.is_adult, l.cover_url, l.created_at,
                u.wallet_address, u.display_name, u.avatar_url,
                COALESCE((SELECT SUM(amount::float8) FROM tips t
                          WHERE t.live_id = l.id AND t.currency = 'YEET'), 0.0)
                  AS tip_total_yeet
           FROM lives l
           JOIN users u ON u.id = l.host_user_id
          WHERE l.host_user_id = $1
            AND (l.status IN ('scheduled','live')
                 OR l.ended_at > NOW() - INTERVAL '7 days')
          ORDER BY
            CASE l.status WHEN 'live' THEN 0 WHEN 'scheduled' THEN 1 ELSE 2 END,
            COALESCE(l.scheduled_for, l.created_at) ASC"
    )
    .bind(user_id)
    .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(rows.into_iter().map(row_to_summary).collect())))
}
