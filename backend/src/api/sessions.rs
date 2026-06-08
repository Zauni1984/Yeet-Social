//! Session + device management.
//!
//! Sessions correspond 1:1 to refresh-token families. Each successful
//! login mints a new family; each `/auth/refresh` rotates within that
//! family. The currently-active row in a family is the one whose
//! `rotated_to_jti` and `revoked_at` are both NULL.
//!
//! User-visible surface:
//!   GET    /api/v1/me/sessions          — list my active sessions
//!   DELETE /api/v1/me/sessions/:id      — revoke one (logs that device out)
//!   DELETE /api/v1/me/sessions          — revoke ALL (force-logout everywhere)
//!
//! Internal surface (called from auth handlers, not directly routed):
//!   record_login() — write a new session row at login time
//!   rotate_refresh() — verify + rotate a refresh JTI; detects reuse
//!                     and revokes the entire family on suspicion
//!   blacklist_session_jti() — blacklist a single JTI in Redis up to
//!                     its TTL so the access middleware rejects it
//!
//! Why refresh-token reuse detection matters:
//! - An attacker who exfiltrates a refresh token can use it to mint
//!   fresh access tokens forever. Without rotation, the only signal we
//!   have is "old refresh still works" — which is the design.
//! - Rotation means each refresh invalidates the prior. If both the
//!   attacker AND the legitimate client present the same JTI, exactly
//!   one wins; the other's next attempt sees a row that's already
//!   rotated. That's our smoking gun: someone replayed → revoke the
//!   whole family and force re-login on all of them.

use axum::{extract::{Path, State}, Json};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;
use crate::api::conversations::caller_user_id;
use crate::services::cache::Cache;

#[derive(Debug, Serialize)]
pub struct SessionRow {
    pub id: Uuid,
    pub device_label: Option<String>,
    pub ip_country: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    /// True for the session that issued the access token making this
    /// request — the client can mark it "this device" in the list.
    pub is_current: bool,
}

pub async fn list_mine(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<Vec<SessionRow>>>> {
    let me = caller_user_id(&state, &auth).await?;
    let rows: Vec<(Uuid, Option<String>, Option<String>, DateTime<Utc>, DateTime<Utc>, String)> =
        sqlx::query_as(
            "SELECT id, device_label, ip_country, created_at, last_seen_at, jti
               FROM user_sessions
              WHERE user_id = $1
                AND revoked_at IS NULL
                AND rotated_to_jti IS NULL
              ORDER BY last_seen_at DESC
              LIMIT 100"
        )
        .bind(me)
        .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;

    // We don't actually know the *refresh* JTI on the current request
    // (the AuthUser carries the *access* JTI). Best-effort: mark the
    // most-recently-touched session as current. Good enough for the UX
    // and avoids surfacing the access JTI in the list response.
    let mut out: Vec<SessionRow> = rows.into_iter().map(|r| SessionRow {
        id: r.0, device_label: r.1, ip_country: r.2,
        created_at: r.3, last_seen_at: r.4, is_current: false,
    }).collect();
    if let Some(first) = out.first_mut() {
        first.is_current = true;
    }
    Ok(Json(ApiResponse::ok(out)))
}

pub async fn revoke_one(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(session_id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    let me = caller_user_id(&state, &auth).await?;
    let row: Option<String> = sqlx::query_scalar(
        "SELECT jti FROM user_sessions
          WHERE id = $1 AND user_id = $2
            AND revoked_at IS NULL"
    )
    .bind(session_id).bind(me)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let jti = row.ok_or_else(|| AppError::NotFound("Session not found".into()))?;

    sqlx::query(
        "UPDATE user_sessions
            SET revoked_at = NOW(), revoked_reason = 'user_revoke'
          WHERE id = $1"
    )
    .bind(session_id)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;

    // Belt-and-braces: blacklist the refresh JTI in Redis so reissue
    // can't sneak through during the small window between DB update
    // and the next refresh attempt.
    blacklist_session_jti(&state.cache, &jti, state.jwt.refresh_ttl_secs).await;
    Ok(Json(ApiResponse::ok("revoked")))
}

pub async fn revoke_all(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    let me = caller_user_id(&state, &auth).await?;
    let jtis: Vec<String> = sqlx::query_scalar(
        "SELECT jti FROM user_sessions
          WHERE user_id = $1
            AND revoked_at IS NULL"
    )
    .bind(me)
    .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;

    sqlx::query(
        "UPDATE user_sessions
            SET revoked_at = NOW(), revoked_reason = 'user_revoke_all'
          WHERE user_id = $1 AND revoked_at IS NULL"
    )
    .bind(me)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;

    for jti in jtis {
        blacklist_session_jti(&state.cache, &jti, state.jwt.refresh_ttl_secs).await;
    }
    Ok(Json(ApiResponse::ok("revoked_all")))
}


// ─── Internal helpers (called from auth handlers) ──────────────────────

pub async fn record_login(
    pool: &PgPool,
    user_id: Uuid,
    refresh_jti: &str,
    device_label: Option<&str>,
    ip_country: Option<&str>,
) -> Result<(), sqlx::Error> {
    let family_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO user_sessions
            (user_id, jti, family_id, device_label, ip_country)
         VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(user_id).bind(refresh_jti).bind(family_id)
    .bind(device_label).bind(ip_country)
    .execute(pool).await?;
    Ok(())
}

/// Outcome of consuming a refresh-token JTI.
pub enum RefreshOutcome {
    /// The refresh was valid and freshly rotated.
    Ok { user_id: Uuid, family_id: Uuid },
    /// The presented JTI was already rotated or revoked. Caller should
    /// revoke the entire family and reject the request.
    Reuse { family_id: Uuid },
    /// The JTI doesn't correspond to any session row — either the
    /// session predates this hardening migration, the token is forged,
    /// or the row was hard-deleted. Treat as a normal auth failure.
    Unknown,
}

/// Atomic refresh-token rotation. Looks up the presented JTI, refuses
/// reuse, otherwise marks the old row rotated_to_jti=new_jti and
/// inserts a new row in the same family.
pub async fn rotate_refresh(
    pool: &PgPool,
    presented_jti: &str,
    new_jti: &str,
) -> AppResult<RefreshOutcome> {
    let mut tx = pool.begin().await.map_err(AppError::Database)?;

    // FOR UPDATE so two concurrent refresh attempts can't both pass.
    let row: Option<(Uuid, Uuid, Uuid, Option<String>, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT id, user_id, family_id, rotated_to_jti, revoked_at
           FROM user_sessions
          WHERE jti = $1
          FOR UPDATE"
    )
    .bind(presented_jti)
    .fetch_optional(&mut *tx).await.map_err(AppError::Database)?;

    let Some((session_id, user_id, family_id, rotated_to_jti, revoked_at)) = row else {
        tx.rollback().await.map_err(AppError::Database)?;
        return Ok(RefreshOutcome::Unknown);
    };

    if rotated_to_jti.is_some() || revoked_at.is_some() {
        tx.rollback().await.map_err(AppError::Database)?;
        return Ok(RefreshOutcome::Reuse { family_id });
    }

    sqlx::query(
        "UPDATE user_sessions
            SET rotated_to_jti = $2, last_seen_at = NOW()
          WHERE id = $1"
    )
    .bind(session_id).bind(new_jti)
    .execute(&mut *tx).await.map_err(AppError::Database)?;

    sqlx::query(
        "INSERT INTO user_sessions
            (user_id, jti, family_id, parent_jti, device_label, ip_country)
         SELECT user_id, $2, family_id, $3, device_label, ip_country
           FROM user_sessions
          WHERE id = $1"
    )
    .bind(session_id).bind(new_jti).bind(presented_jti)
    .execute(&mut *tx).await.map_err(AppError::Database)?;

    tx.commit().await.map_err(AppError::Database)?;
    Ok(RefreshOutcome::Ok { user_id, family_id })
}

/// Mark every session in a family as revoked. Called when refresh
/// reuse is detected.
pub async fn revoke_family(
    pool: &PgPool,
    cache: &Cache,
    family_id: Uuid,
    refresh_ttl: u64,
) -> Result<(), sqlx::Error> {
    let jtis: Vec<String> = sqlx::query_scalar(
        "SELECT jti FROM user_sessions
          WHERE family_id = $1 AND revoked_at IS NULL"
    )
    .bind(family_id)
    .fetch_all(pool).await?;

    sqlx::query(
        "UPDATE user_sessions
            SET revoked_at = NOW(), revoked_reason = 'refresh_reuse'
          WHERE family_id = $1 AND revoked_at IS NULL"
    )
    .bind(family_id)
    .execute(pool).await?;

    for jti in jtis {
        blacklist_session_jti(cache, &jti, refresh_ttl).await;
    }
    Ok(())
}

pub async fn blacklist_session_jti(cache: &Cache, jti: &str, ttl_secs: u64) {
    let _ = cache.blacklist_token(jti, Duration::from_secs(ttl_secs)).await;
}
