//! Tip handlers.
use axum::{extract::State, Json};
use serde::Deserialize;
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;

#[derive(Debug, Deserialize)]
pub struct SendTipRequest {
    pub to_address: String,
    pub amount: String,
    pub currency: String,
    pub post_id: Option<Uuid>,
    pub tx_hash: Option<String>,
}

pub async fn send_tip(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<SendTipRequest>,
) -> AppResult<Json<ApiResponse<Uuid>>> {
    if !["BNB", "YEET"].contains(&req.currency.as_str()) {
        return Err(AppError::Validation("Currency must be BNB or YEET".into()));
    }

    // Support both wallet-login (wallet_address) and email-login (email:UUID in sub)
    let from_id: Uuid = if auth.address.starts_with("email:") {
        let id_str = auth.address.trim_start_matches("email:");
        Uuid::parse_str(id_str).map_err(|_| AppError::Validation("Invalid user id".into()))?
    } else {
        sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
            .bind(&auth.address)
            .fetch_optional(state.db.pool())
            .await
            .map_err(AppError::Database)?
            .ok_or_else(|| AppError::NotFound("Sender not found".into()))?
    };

    // Find recipient by wallet_address or by id if to_address looks like a UUID
    let to_id: Uuid = if let Ok(uid) = Uuid::parse_str(&req.to_address) {
        uid
    } else {
        sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
            .bind(req.to_address.to_lowercase())
            .fetch_optional(state.db.pool())
            .await
            .map_err(AppError::Database)?
            .ok_or_else(|| AppError::NotFound("Recipient not found".into()))?
    };

    if from_id == to_id {
        return Err(AppError::Validation("Cannot tip yourself".into()));
    }

    let tip_id: Uuid = sqlx::query_scalar(
        "INSERT INTO tips (from_user_id, to_user_id, post_id, amount, currency, platform_fee, tx_hash)
         VALUES ($1, $2, $3, $4, $5, '0.10', $6) RETURNING id"
    )
    .bind(from_id)
    .bind(to_id)
    .bind(req.post_id)
    .bind(&req.amount)
    .bind(&req.currency)
    .bind(&req.tx_hash)
    .fetch_one(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(tip_id)))
}
