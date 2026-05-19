//! Token balance + rewards handlers.
use axum::{extract::State, Json};
use serde::Serialize;
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, models::ApiResponse, services::tokens};
use crate::api::middleware::AuthUser;

#[derive(Debug, Serialize)]
pub struct BalanceResponse {
    /// Spendable YEET Credit (off-chain, custodial). Funds paper
    /// wallets, PPV unlocks, and DM-attached tips. Distinct from the
    /// user's on-chain YEET balance, which lives at `wallet_address`
    /// and is queried browser-side via `balanceOf` on the YEET BEP-20
    /// contract.
    pub credit_balance: f64,
    /// Reward-pool credits that haven't been minted on-chain yet.
    pub pending_yeet: i64,
    pub wallet_address: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RewardsResponse { pub total_earned: i64, pub daily_limit: i64, pub daily_remaining: i64 }

/// Resolve the caller's user id from the JWT subject. Supports both
/// wallet (`0x...`) and email (`email:<uuid>`) auth styles, otherwise
/// email-registered accounts would get a 404 here.
async fn caller_user_id(state: &AppState, auth: &AuthUser) -> AppResult<Uuid> {
    if let Some(rest) = auth.address.strip_prefix("email:") {
        return Uuid::parse_str(rest)
            .map_err(|_| AppError::Validation("Invalid user id".into()));
    }
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE wallet_address = $1")
        .bind(&auth.address)
        .fetch_optional(state.db.pool())
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))
}

pub async fn get_balance(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<BalanceResponse>>> {
    let user_id = caller_user_id(&state, &auth).await?;

    let row: (f64, Option<String>) = sqlx::query_as(
        "SELECT COALESCE(yeet_credit_balance, 0)::float8 AS credit_balance, wallet_address
           FROM users WHERE id = $1"
    )
    .bind(user_id)
    .fetch_one(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    let pending = tokens::get_pending_balance(&state.db, user_id).await?;

    Ok(Json(ApiResponse::ok(BalanceResponse {
        credit_balance: row.0,
        pending_yeet: pending,
        wallet_address: row.1,
    })))
}

pub async fn get_rewards(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<RewardsResponse>>> {
    let user_id = caller_user_id(&state, &auth).await?;
    let total_earned: i64 = sqlx::query_scalar(
        "SELECT COALESCE(total_yeet_earned, 0) FROM users WHERE id = $1"
    )
    .bind(user_id).fetch_one(state.db.pool()).await.map_err(AppError::Database)?;

    let today_earned: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount), 0) FROM token_rewards WHERE user_id = $1 AND created_at >= CURRENT_DATE"
    )
    .bind(user_id).fetch_one(state.db.pool()).await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(RewardsResponse {
        total_earned,
        daily_limit: tokens::rewards::DAILY_CAP,
        daily_remaining: (tokens::rewards::DAILY_CAP - today_earned).max(0),
    })))
}
