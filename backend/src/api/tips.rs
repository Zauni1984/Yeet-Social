//! Tip handlers.
//!
//! The off-chain tip path is implemented as `send_tip_tx` which takes a
//! caller-owned transaction. The HTTP route wraps it in `begin()`; the
//! DM tip-message path (`api::messages`) joins the same tx so that the
//! tip record + the message row land atomically.
use axum::{extract::State, Json};
use serde::Deserialize;
use sqlx::{Postgres, Transaction};
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

/// Inserts the tip + fee ledger entry and adjusts the sender / fee
/// wallet balances *inside the caller's transaction*. Returns the new
/// tip's id.
///
/// Pre-conditions enforced inline:
/// - sender has enough YEET
/// - sender != recipient
/// - currency is BNB or YEET
pub(crate) async fn send_tip_tx(
    tx: &mut Transaction<'_, Postgres>,
    from_id: Uuid,
    to_id: Uuid,
    post_id: Option<Uuid>,
    amount_str: &str,
    currency: &str,
    tx_hash: Option<&str>,
) -> AppResult<Uuid> {
    if !["BNB", "YEET"].contains(&currency) {
        return Err(AppError::Validation("Currency must be BNB or YEET".into()));
    }
    let amount_val: f64 = amount_str.parse().unwrap_or(0.0);
    if amount_val <= 0.0 {
        return Err(AppError::Validation("Amount must be greater than 0".into()));
    }
    if from_id == to_id {
        return Err(AppError::Validation("Cannot tip yourself".into()));
    }

    // Lock sender row to prevent concurrent over-spend.
    let balance: f64 = sqlx::query_scalar(
        "SELECT COALESCE(yeet_token_balance, 0)::float8 FROM users WHERE id = $1 FOR UPDATE"
    )
    .bind(from_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::Database)?;
    if balance < amount_val {
        return Err(AppError::Validation("Insufficient points".into()));
    }

    let creator_amount = amount_val * 0.9;
    let platform_cut   = amount_val * 0.1;

    let tip_id: Uuid = sqlx::query_scalar(
        "INSERT INTO tips (from_user_id, to_user_id, post_id, amount, creator_amount, platform_cut, currency, tx_hash) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id"
    )
    .bind(from_id).bind(to_id).bind(post_id)
    .bind(amount_str)
    .bind(creator_amount.to_string()).bind(platform_cut.to_string())
    .bind(currency).bind(tx_hash)
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::Database)?;

    sqlx::query(
        "INSERT INTO fee_ledger (source_type, source_id, gross_amount, fee_amount, creator_amount)
         VALUES ('tip', $1, $2, $3, $4)"
    )
    .bind(tip_id).bind(amount_val).bind(platform_cut).bind(creator_amount)
    .execute(&mut **tx).await.map_err(AppError::Database)?;

    sqlx::query(
        "UPDATE fee_wallet_balance SET total_yeet = total_yeet + $1 WHERE id = 1"
    )
    .bind(platform_cut)
    .execute(&mut **tx).await.map_err(AppError::Database)?;

    sqlx::query(
        "UPDATE users SET yeet_token_balance = yeet_token_balance - $1 WHERE id = $2"
    )
    .bind(amount_val).bind(from_id)
    .execute(&mut **tx).await.map_err(AppError::Database)?;

    // Credit the recipient for YEET tips (off-chain ledger).
    if currency == "YEET" {
        sqlx::query(
            "UPDATE users SET yeet_token_balance = yeet_token_balance + $1 WHERE id = $2"
        )
        .bind(creator_amount).bind(to_id)
        .execute(&mut **tx).await.map_err(AppError::Database)?;
    }

    Ok(tip_id)
}

pub async fn send_tip(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<SendTipRequest>,
) -> AppResult<Json<ApiResponse<Uuid>>> {
    // Resolve sender id
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

    // Resolve recipient (UUID or wallet)
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

    // Refuse if either party has blocked the other.
    if crate::api::blocks::either_blocks(state.db.pool(), from_id, to_id).await? {
        return Err(AppError::Forbidden("Blocked".into()));
    }

    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;
    let tip_id = send_tip_tx(
        &mut tx, from_id, to_id, req.post_id,
        &req.amount, &req.currency, req.tx_hash.as_deref(),
    ).await?;
    tx.commit().await.map_err(AppError::Database)?;

    // Notify recipient after the tx has settled. Best-effort.
    let actor = sqlx::query_scalar::<_, Option<String>>(
        "SELECT COALESCE(display_name, username) FROM users WHERE id = $1"
    ).bind(from_id).fetch_optional(state.db.pool()).await
     .ok().flatten().flatten().unwrap_or_else(|| "Someone".into());
    crate::api::notifications::notify(
        state.db.pool(), to_id, Some(from_id),
        "tip",
        &format!("{} tipped you {} {}", actor, req.amount, req.currency),
        req.post_id,
    ).await;

    Ok(Json(ApiResponse::ok(tip_id)))
}
