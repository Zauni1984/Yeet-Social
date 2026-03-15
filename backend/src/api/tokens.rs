//! Token balance + rewards handlers.
use axum::{extract::State, Json};
use serde::Serialize;
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, models::ApiResponse, services::tokens};
use crate::api::middleware::AuthUser;

#[derive(Debug, Serialize)]
pub struct BalanceResponse { pub pending_yeet: i64, pub wallet_address: String }

#[derive(Debug, Serialize)]
pub struct RewardsResponse { pub total_earned: i64, pub daily_limit: i64, pub daily_remaining: i64 }

pub async fn get_balance(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<BalanceResponse>>> {
    let user_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
        .bind(&auth.address).fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))?;
    let pending = tokens::get_pending_balance(&state.db, user_id).await?;
    Ok(Json(ApiResponse::ok(BalanceResponse { pending_yeet: pending, wallet_address: auth.address })))
}

pub async fn get_rewards(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<RewardsResponse>>> {
    let (user_id, total_earned): (Uuid, i64) = sqlx::query_as(
        "SELECT id, total_yeet_earned FROM users WHERE wallet_address = $1"
    )
    .bind(&auth.address).fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
    .ok_or_else(|| AppError::NotFound("User not found".into()))?;

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
