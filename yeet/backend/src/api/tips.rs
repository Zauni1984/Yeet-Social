use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use uuid::Uuid;
use crate::AppState;
use shared::{ApiResponse, Tip, TipCurrency};
use chrono::Utc;

#[derive(Deserialize)]
pub struct TipRequest {
    pub post_id: Uuid,
    pub to_user_id: Uuid,
    pub amount: f64,
    pub currency: TipCurrency,
    pub tx_hash: Option<String>, // BSC tx hash for crypto tips
}

pub async fn send_tip(
    State(state): State<AppState>,
    Json(req): Json<TipRequest>,
) -> Result<Json<ApiResponse<Tip>>, StatusCode> {
    let from_user_id = Uuid::new_v4(); // TODO: from JWT

    // For crypto tips: verify BSC tx before recording
    if let Some(ref hash) = req.tx_hash {
        let confirmed = state.bsc.get_tx_status(hash).await
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        if !confirmed {
            return Ok(Json(ApiResponse::err("BSC transaction not confirmed")));
        }
    }

    // Calculate platform cut
    // 10% for crypto, 20% for fiat
    let platform_cut = match req.currency {
        TipCurrency::Yeet | TipCurrency::Bnb => req.amount * 0.10,
        TipCurrency::Fiat => req.amount * 0.20,
    };
    let creator_amount = req.amount - platform_cut;

    // Record tip in DB
    let tip = sqlx::query!(
        r#"
        INSERT INTO tips (id, from_user_id, to_user_id, post_id,
            amount, creator_amount, platform_cut, currency, tx_hash, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8::tip_currency, $9, NOW())
        RETURNING id, created_at
        "#,
        Uuid::new_v4(),
        from_user_id,
        req.to_user_id,
        req.post_id,
        req.amount,
        creator_amount,
        platform_cut,
        serde_json::to_string(&req.currency).unwrap().trim_matches('"'),
        req.tx_hash,
    )
    .fetch_one(&state.db.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Update post tip_total
    sqlx::query!(
        "UPDATE posts SET tip_total = tip_total + $1 WHERE id = $2",
        creator_amount, req.post_id
    )
    .execute(&state.db.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(ApiResponse::ok(Tip {
        id: tip.id,
        from_user_id,
        to_user_id: req.to_user_id,
        post_id: req.post_id,
        amount: req.amount,
        currency: req.currency,
        tx_hash: req.tx_hash,
        created_at: tip.created_at,
    })))
}
