//! Tip handlers — crypto tipping between users.
use axum::{extract::State, Json};
use serde::Deserialize;
use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;
use uuid::Uuid;

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
    let from = sqlx::query!("SELECT id FROM users WHERE wallet_address = $1", auth.address)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("Sender not found".into()))?;
    let to = sqlx::query!("SELECT id FROM users WHERE wallet_address = $1", req.to_address.to_lowercase())
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("Recipient not found".into()))?;

    if from.id == to.id { return Err(AppError::Validation("Cannot tip yourself".into())); }

    if !["BNB", "YEET"].contains(&req.currency.as_str()) {
        return Err(AppError::Validation("Currency must be BNB or YEET".into()));
    }

    // Platform fee: 10% crypto, 20% fiat (both are crypto here)
    let platform_fee_pct = if req.currency == "BNB" { "0.10" } else { "0.10" };

    let tip_id = sqlx::query_scalar!(
        r#"INSERT INTO tips (from_user_id, to_user_id, post_id, amount, currency, platform_fee, tx_hash)
           VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id"#,
        from.id, to.id, req.post_id, req.amount, req.currency, platform_fee_pct, req.tx_hash
    )
    .fetch_one(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(tip_id)))
}
