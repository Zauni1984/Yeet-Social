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

    let amount_val: f64 = req.amount.parse().unwrap_or(0.0);
    if amount_val <= 0.0 {
        return Err(AppError::Validation("Amount must be greater than 0".into()));
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

    // Check sender balance
    let balance: f64 = sqlx::query_scalar(
        "SELECT COALESCE(yeet_token_balance, 0)::float8 FROM users WHERE id = $1"
    )
    .bind(from_id)
    .fetch_one(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    if balance < amount_val {
        return Err(AppError::Validation("Insufficient YEET balance".into()));
    }

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

    let creator_amount = amount_val * 0.9;
    let platform_cut = amount_val * 0.1;

    let tip_id: Uuid = sqlx::query_scalar(
        "INSERT INTO tips (from_user_id, to_user_id, post_id, amount, creator_amount, platform_cut, currency, tx_hash) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id"
    )
    .bind(from_id)
    .bind(to_id)
    .bind(req.post_id)
    .bind(&req.amount)
    .bind(creator_amount.to_string())
    .bind(platform_cut.to_string())
    .bind(&req.currency)
    .bind(&req.tx_hash)
    .fetch_one(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    // Record fee to ledger
    let _ = sqlx::query(
        "INSERT INTO fee_ledger (source_type, source_id, gross_amount, fee_amount, creator_amount)
         VALUES ('tip', $1, $2, $3, $4)"
    )
    .bind(tip_id).bind(amount).bind(platform_cut).bind(creator_amount)
    .execute(state.db.pool()).await;

    // Update fee wallet balance
    let _ = sqlx::query(
        "UPDATE fee_wallet_balance SET total_yeet = total_yeet + $1 WHERE id = 1"
    )
    .bind(platform_cut)
    .execute(state.db.pool()).await;

    // Debit sender balance
    sqlx::query(
        "UPDATE users SET yeet_token_balance = yeet_token_balance - $1 WHERE id = $2"
    )
    .bind(amount_val)
    .bind(from_id)
    .execute(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(tip_id)))
}
