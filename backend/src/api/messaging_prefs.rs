//! Messaging privacy preferences.
//!
//! Lives separately from the general user_settings table because the
//! two toggles here are columns on `users` (added in migration 0031)
//! and have semantics tighter than the typical setting:
//!
//!  * `read_receipts_enabled` is symmetric: when a user turns it off
//!    we both stop writing their own read receipts AND filter other
//!    users' read receipts out of any reply the server makes to them.
//!    The opt-out therefore cannot be used to "peek" at others.
//!  * `typing_indicators_enabled` is a pure UX toggle today — the
//!    realtime layer isn't shipped yet but the column is already in
//!    the DB so the surface is stable for when WS lands.
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;
use crate::api::conversations::caller_user_id;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct MessagingPrefs {
    pub read_receipts_enabled: bool,
    pub typing_indicators_enabled: bool,
}

pub async fn get_prefs(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<MessagingPrefs>>> {
    let me = caller_user_id(&state, &auth).await?;
    let row = sqlx::query_as::<_, MessagingPrefs>(
        "SELECT read_receipts_enabled, typing_indicators_enabled FROM users WHERE id = $1"
    )
    .bind(me)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
    .ok_or_else(|| AppError::NotFound("User not found".into()))?;
    Ok(Json(ApiResponse::ok(row)))
}

#[derive(Debug, Deserialize)]
pub struct UpdatePrefsRequest {
    pub read_receipts_enabled: Option<bool>,
    pub typing_indicators_enabled: Option<bool>,
}

pub async fn update_prefs(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<UpdatePrefsRequest>,
) -> AppResult<Json<ApiResponse<MessagingPrefs>>> {
    let me = caller_user_id(&state, &auth).await?;
    // COALESCE so a PATCH with only one field set leaves the other
    // untouched. Both columns are NOT NULL with explicit defaults so
    // the SELECT after the UPDATE is guaranteed to return a row.
    sqlx::query(
        "UPDATE users
            SET read_receipts_enabled     = COALESCE($2, read_receipts_enabled),
                typing_indicators_enabled = COALESCE($3, typing_indicators_enabled),
                updated_at                = NOW()
          WHERE id = $1"
    )
    .bind(me)
    .bind(req.read_receipts_enabled)
    .bind(req.typing_indicators_enabled)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;

    get_prefs(State(state), auth).await
}
