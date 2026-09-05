//! NOTE → YEET swap API (see services::note_swap + docs/swap-note-to-yeet.md).
//!
//! Public status is always served (the swap page shows the lock state from
//! it); the user-facing endpoints refuse with SWAP_LOCKED until the swap is
//! switched on, and then require KYC + a linked payout wallet.
use axum::{extract::State, Json};
use serde::Serialize;
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;
use crate::services::note_swap::{self, NOTE_PER_YEET};

#[derive(Debug, Serialize)]
pub struct SwapStatus {
    /// Swaps accepted right now?
    pub enabled: bool,
    /// NOTE per 1 YEET (fixed).
    pub note_per_yeet: f64,
    pub confirmations_required: i64,
    pub pool_cap_yeet: f64,
    /// YEET already queued/paid through the swap (excl. rejected).
    pub swapped_yeet: f64,
    pub min_note: f64,
}

#[derive(Debug, Serialize)]
pub struct SwapAddress { pub note_address: String, pub note_per_yeet: f64, pub confirmations_required: i64 }

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SwapDepositRow {
    pub id: Uuid,
    pub txid: String,
    pub vout: i32,
    pub amount_note: f64,
    pub confirmations: i32,
    pub status: String,
    pub payout_id: Option<Uuid>,
    pub last_error: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

async fn caller_user_id(state: &AppState, auth: &AuthUser) -> AppResult<Uuid> {
    if let Some(rest) = auth.address.strip_prefix("email:") {
        return Uuid::parse_str(rest).map_err(|_| AppError::Validation("Invalid user id".into()));
    }
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE wallet_address = $1")
        .bind(&auth.address)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))
}

/// GET /api/v1/swap/status — public.
pub async fn status(State(state): State<AppState>) -> AppResult<Json<ApiResponse<SwapStatus>>> {
    let cfg = note_swap::config();
    let swapped: f64 = sqlx::query_scalar::<_, f64>(
        "SELECT COALESCE(SUM(amount), 0)::float8 FROM token_rewards
          WHERE kind = 'conversion' AND action = 'note_swap' AND status <> 'rejected'"
    ).fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(SwapStatus {
        enabled: note_swap::is_live(&cfg),
        note_per_yeet: NOTE_PER_YEET,
        confirmations_required: cfg.confirmations,
        pool_cap_yeet: cfg.pool_cap_yeet,
        swapped_yeet: swapped,
        min_note: cfg.min_note,
    })))
}

/// POST /api/v1/swap/address — the caller's personal NOTE deposit address.
/// Locked until the swap is live; then requires KYC + linked payout wallet.
pub async fn address(State(state): State<AppState>, auth: AuthUser) -> AppResult<Json<ApiResponse<SwapAddress>>> {
    let cfg = note_swap::config();
    if !note_swap::is_live(&cfg) {
        return Err(AppError::Forbidden("SWAP_LOCKED".into()));
    }
    let user_id = caller_user_id(&state, &auth).await?;
    let (kyc, wallet): (Option<chrono::DateTime<chrono::Utc>>, Option<String>) = sqlx::query_as(
        "SELECT age_verified_at, wallet_address FROM users WHERE id = $1"
    ).bind(user_id).fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
    if kyc.is_none() { return Err(AppError::Forbidden("KYC_REQUIRED".into())); }
    if wallet.is_none() { return Err(AppError::Forbidden("NO_WALLET_LINKED".into())); }

    let note_address = note_swap::allocate_address(&state.db, &cfg, user_id).await?;
    Ok(Json(ApiResponse::ok(SwapAddress {
        note_address, note_per_yeet: NOTE_PER_YEET, confirmations_required: cfg.confirmations,
    })))
}

/// GET /api/v1/swap/deposits — the caller's own deposits and their state.
pub async fn deposits(State(state): State<AppState>, auth: AuthUser) -> AppResult<Json<ApiResponse<Vec<SwapDepositRow>>>> {
    let user_id = caller_user_id(&state, &auth).await?;
    let rows = sqlx::query_as::<_, SwapDepositRow>(
        "SELECT id, txid, vout, amount_note::float8 AS amount_note, confirmations, status, payout_id, last_error, created_at
           FROM swap_deposits WHERE user_id = $1 ORDER BY created_at DESC LIMIT 200"
    ).bind(user_id).fetch_all(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(rows)))
}
