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

    // Phase 2 will mint a LiveKit room name + token here. For now we
    // wrap the state flip in the same tx that materialises any booked
    // promotion, so the announcement post is guaranteed to appear if
    // and only if the broadcast actually transitions to live.
    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;
    sqlx::query(
        "UPDATE lives
            SET status = 'live',
                started_at = COALESCE(started_at, NOW())
          WHERE id = $1"
    ).bind(id).execute(&mut *tx).await.map_err(AppError::Database)?;

    // Pending promotion? Apply it now (auto-post + optional pin).
    let pending: Option<(Uuid, String, Option<i32>)> = sqlx::query_as(
        "SELECT id, tier, boost_minutes
           FROM live_promotions
          WHERE live_id = $1 AND status = 'booked'
          FOR UPDATE"
    ).bind(id).fetch_optional(&mut *tx).await.map_err(AppError::Database)?;
    if let Some((promo_id, tier, boost_minutes)) = pending {
        apply_promotion_in_tx(&mut tx, id, promo_id, &tier, boost_minutes, user_id).await?;
    }
    tx.commit().await.map_err(AppError::Database)?;

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
    // Refund any booked promotion in the same tx so we never leak the
    // host's YEET on a cancelled broadcast.
    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;
    sqlx::query("UPDATE lives SET status = 'cancelled' WHERE id = $1")
        .bind(id).execute(&mut *tx).await.map_err(AppError::Database)?;
    refund_promotion_in_tx(&mut tx, id).await?;
    tx.commit().await.map_err(AppError::Database)?;
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

// ─── Paid promotions ──────────────────────────────────────────────────────
//
// Hosts can boost their live by paying YEET. Two tiers, fixed prices,
// charged at booking time. If the live is later cancelled, the cost is
// refunded; if it actually starts, an auto-post lands in the public
// feed announcing the broadcast. The 'boost' tier additionally pins
// that post to the top of For-You / Following feeds for 60 minutes.
//
// Cost goes 100% to the platform fee wallet (no creator split) — this
// is ad spend, not a tip.

const PROMO_BASIC_YEET: f64 = 10.0;
const PROMO_BOOST_YEET: f64 = 50.0;
const PROMO_BOOST_MINUTES: i64 = 60;

fn promo_cost(tier: &str) -> AppResult<f64> {
    match tier {
        "basic" => Ok(PROMO_BASIC_YEET),
        "boost" => Ok(PROMO_BOOST_YEET),
        _ => Err(AppError::Validation("tier must be 'basic' or 'boost'".into())),
    }
}

#[derive(Debug, Deserialize)]
pub struct BookPromotionRequest {
    pub tier: String, // 'basic' | 'boost'
}

#[derive(Debug, Serialize)]
pub struct BookPromotionResponse {
    pub promotion_id: Uuid,
    pub tier: String,
    pub cost_yeet: f64,
    pub new_balance: f64,
}

/// Book a paid promotion for an upcoming or already-live broadcast.
/// Charges the host's YEET balance immediately. Idempotent per live:
/// the unique index `uq_live_promotions_active` rejects a second active
/// booking with a clear validation error.
pub async fn book_promotion(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(live_id): Path<Uuid>,
    Json(req): Json<BookPromotionRequest>,
) -> AppResult<Json<ApiResponse<BookPromotionResponse>>> {
    let user_id = resolve_user_id(&state, &auth.address).await?;
    let cost = promo_cost(&req.tier)?;

    // Verify the live exists, belongs to the caller, and is in a
    // bookable state. We allow booking on already-live broadcasts so
    // a host who didn't pre-book can still buy a promo mid-stream;
    // ended/cancelled lives are rejected.
    let row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT host_user_id, status FROM lives WHERE id = $1"
    ).bind(live_id).fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (host_id, status) = row.ok_or_else(|| AppError::NotFound("Live not found".into()))?;
    if host_id != user_id {
        return Err(AppError::Forbidden("Only the host can promote this live".into()));
    }
    if status == "ended" || status == "cancelled" {
        return Err(AppError::Validation("Cannot promote an ended live".into()));
    }

    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;

    // Lock + check balance.
    let balance: f64 = sqlx::query_scalar(
        "SELECT COALESCE(yeet_token_balance, 0)::float8 FROM users WHERE id = $1 FOR UPDATE"
    )
    .bind(user_id).fetch_one(&mut *tx).await.map_err(AppError::Database)?;
    if balance < cost {
        return Err(AppError::Validation(format!(
            "Insufficient YEET balance: have {balance:.2}, need {cost:.2}"
        )));
    }

    // Insert the promo row first so the unique-active index can
    // reject duplicates before we touch any balances.
    let boost_minutes = if req.tier == "boost" { Some(PROMO_BOOST_MINUTES as i32) } else { None };
    let promo_id: Uuid = match sqlx::query_scalar(
        "INSERT INTO live_promotions (live_id, user_id, tier, cost_yeet, boost_minutes)
         VALUES ($1, $2, $3, $4, $5) RETURNING id"
    )
    .bind(live_id).bind(user_id).bind(&req.tier).bind(cost).bind(boost_minutes)
    .fetch_one(&mut *tx).await
    {
        Ok(id) => id,
        Err(sqlx::Error::Database(e)) if e.constraint() == Some("uq_live_promotions_active") => {
            return Err(AppError::Conflict("This live already has an active promotion".into()));
        }
        Err(e) => return Err(AppError::Database(e)),
    };

    // Debit the host, credit the platform fee wallet, ledger entry.
    sqlx::query("UPDATE users SET yeet_token_balance = yeet_token_balance - $1 WHERE id = $2")
        .bind(cost).bind(user_id)
        .execute(&mut *tx).await.map_err(AppError::Database)?;
    sqlx::query("UPDATE fee_wallet_balance SET total_yeet = total_yeet + $1 WHERE id = 1")
        .bind(cost)
        .execute(&mut *tx).await.map_err(AppError::Database)?;
    sqlx::query(
        "INSERT INTO fee_ledger (source_type, source_id, gross_amount, fee_amount, creator_amount)
         VALUES ('live_promo', $1, $2, $2, 0)"
    )
    .bind(promo_id).bind(cost)
    .execute(&mut *tx).await.map_err(AppError::Database)?;

    // If the live is already live, apply the promotion now so the
    // auto-post hits the feed immediately. Otherwise it'll be applied
    // by `start_live`.
    if status == "live" {
        apply_promotion_in_tx(&mut tx, live_id, promo_id, &req.tier, boost_minutes, user_id).await?;
    }

    let new_balance: f64 = sqlx::query_scalar(
        "SELECT COALESCE(yeet_token_balance, 0)::float8 FROM users WHERE id = $1"
    ).bind(user_id).fetch_one(&mut *tx).await.map_err(AppError::Database)?;

    tx.commit().await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(BookPromotionResponse {
        promotion_id: promo_id,
        tier: req.tier,
        cost_yeet: cost,
        new_balance,
    })))
}

/// Inside an open transaction: turn a 'booked' promotion into 'applied'
/// by inserting the announcement post. Called from `start_live` and
/// also from `book_promotion` when the live is already live.
///
/// The auto-post carries `promoted_live_id` so the client can render a
/// "Watch live" CTA card instead of plain text. Boost-tier promos also
/// set `pinned_until` so the feed query lifts them to the top.
async fn apply_promotion_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    live_id: Uuid,
    promo_id: Uuid,
    tier: &str,
    boost_minutes: Option<i32>,
    host_id: Uuid,
) -> AppResult<()> {
    // Pull the live's title for the auto-post body.
    let (title, is_adult): (String, bool) = sqlx::query_as(
        "SELECT title, is_adult FROM lives WHERE id = $1"
    ).bind(live_id).fetch_one(&mut **tx).await.map_err(AppError::Database)?;

    // Trim to fit posts.content's 280-char limit while keeping the
    // emoji + LIVE prefix readable. Title is the only variable bit.
    let prefix = "🔴 LIVE NOW: ";
    let max_title_len = 280usize.saturating_sub(prefix.len());
    let title_clipped: String = title.chars().take(max_title_len).collect();
    let body = format!("{prefix}{title_clipped}");

    let pinned_until = if tier == "boost" {
        let mins = boost_minutes.unwrap_or(PROMO_BOOST_MINUTES as i32) as i64;
        Some(Utc::now() + chrono::Duration::minutes(mins))
    } else {
        None
    };

    // The auto-post lives in the normal feed and follows the usual 24h
    // expiry so it dies cleanly even if the host disappears.
    let post_id: Uuid = sqlx::query_scalar(
        "INSERT INTO posts
           (author_id, content, media_urls, expires_at, is_adult,
            promoted_live_id, pinned_until)
         VALUES ($1, $2, $3, NOW() + INTERVAL '24 hours', $4, $5, $6)
         RETURNING id"
    )
    .bind(host_id).bind(&body).bind(Vec::<String>::new())
    .bind(is_adult).bind(live_id).bind(pinned_until)
    .fetch_one(&mut **tx).await.map_err(AppError::Database)?;

    sqlx::query(
        "UPDATE live_promotions
            SET status = 'applied', applied_at = NOW(), auto_post_id = $1
          WHERE id = $2"
    )
    .bind(post_id).bind(promo_id)
    .execute(&mut **tx).await.map_err(AppError::Database)?;

    Ok(())
}

/// Refund a booked-but-not-yet-applied promotion. Called from
/// `cancel_live`. Best-effort: if there's no booked promo, this is a
/// no-op so callers don't have to check first.
async fn refund_promotion_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    live_id: Uuid,
) -> AppResult<()> {
    let promo: Option<(Uuid, Uuid, f64)> = sqlx::query_as(
        "SELECT id, user_id, cost_yeet::float8
           FROM live_promotions
          WHERE live_id = $1 AND status = 'booked'
          FOR UPDATE"
    ).bind(live_id).fetch_optional(&mut **tx).await.map_err(AppError::Database)?;
    let Some((promo_id, user_id, cost)) = promo else { return Ok(()); };

    sqlx::query("UPDATE users SET yeet_token_balance = yeet_token_balance + $1 WHERE id = $2")
        .bind(cost).bind(user_id)
        .execute(&mut **tx).await.map_err(AppError::Database)?;
    sqlx::query("UPDATE fee_wallet_balance SET total_yeet = total_yeet - $1 WHERE id = 1")
        .bind(cost)
        .execute(&mut **tx).await.map_err(AppError::Database)?;
    sqlx::query(
        "INSERT INTO fee_ledger (source_type, source_id, gross_amount, fee_amount, creator_amount)
         VALUES ('live_promo_refund', $1, $2, $2, 0)"
    )
    .bind(promo_id).bind(-cost)
    .execute(&mut **tx).await.map_err(AppError::Database)?;

    sqlx::query("UPDATE live_promotions SET status = 'refunded', refunded_at = NOW() WHERE id = $1")
        .bind(promo_id)
        .execute(&mut **tx).await.map_err(AppError::Database)?;
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct PromotionInfo {
    pub id: Uuid,
    pub tier: String,
    pub cost_yeet: f64,
    pub status: String,
    pub boost_minutes: Option<i32>,
    pub auto_post_id: Option<Uuid>,
    pub booked_at: DateTime<Utc>,
}

pub async fn get_promotion(
    State(state): State<AppState>,
    Path(live_id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<Option<PromotionInfo>>>> {
    let row: Option<(Uuid, String, f64, String, Option<i32>, Option<Uuid>, DateTime<Utc>)> =
        sqlx::query_as(
            "SELECT id, tier, cost_yeet::float8, status, boost_minutes, auto_post_id, booked_at
               FROM live_promotions
              WHERE live_id = $1 AND status IN ('booked','applied')
              ORDER BY booked_at DESC LIMIT 1"
        ).bind(live_id).fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let info = row.map(|(id, tier, cost_yeet, status, boost_minutes, auto_post_id, booked_at)| {
        PromotionInfo { id, tier, cost_yeet, status, boost_minutes, auto_post_id, booked_at }
    });
    Ok(Json(ApiResponse::ok(info)))
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
