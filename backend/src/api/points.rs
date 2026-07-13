//! Points → YEET one-way conversion (docs/mica/05).
//!
//! Email users earn off-chain POINTS (`users.yeet_token_balance`). This is the
//! ONLY bridge from points to on-chain YEET, and it is strictly one-way:
//! points are debited and a payout row (kind='conversion') is queued for the
//! batch minter, which pays YEET to the user's VERIFIED EXTERNAL wallet. There
//! is deliberately no reverse endpoint (YEET can never be turned back into
//! points), so the platform never takes custody of anyone's crypto.
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;

/// Minimum points per conversion (keeps on-chain gas economical + avoids dust).
const MIN_CONVERT_POINTS: i64 = 100;

#[derive(Debug, Deserialize)]
pub struct ConvertRequest {
    /// Whole points to convert to YEET (1 point = 1 YEET, 1:1). Integer.
    pub points: i64,
}

#[derive(Debug, Serialize)]
pub struct ConvertResponse {
    pub converted_points: i64,
    pub payout_id: Uuid,
    pub wallet_address: String,
    /// Remaining spendable points after the debit.
    pub points_balance: f64,
    pub status: &'static str,
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

/// POST /api/v1/points/convert
pub async fn convert(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<ConvertRequest>,
) -> AppResult<Json<ApiResponse<ConvertResponse>>> {
    if req.points < MIN_CONVERT_POINTS {
        return Err(AppError::Validation(format!(
            "Minimum conversion is {MIN_CONVERT_POINTS} points"
        )));
    }
    let user_id = caller_user_id(&state, &auth).await?;

    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;

    // The payout target MUST be a verified EXTERNAL wallet the user linked via
    // the signature-challenge flow (email_auth::link_wallet_verify). Email
    // users without a linked wallet cannot convert — they connect one first.
    let (balance, wallet): (f64, Option<String>) = sqlx::query_as(
        "SELECT COALESCE(yeet_token_balance, 0)::float8, wallet_address
           FROM users WHERE id = $1 FOR UPDATE"
    )
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    let wallet = wallet.ok_or_else(|| {
        AppError::Forbidden("NO_WALLET_LINKED".into())
    })?;

    if balance < req.points as f64 {
        return Err(AppError::Validation("Insufficient points".into()));
    }

    // Debit points (one-way).
    sqlx::query("UPDATE users SET yeet_token_balance = yeet_token_balance - $1 WHERE id = $2")
        .bind(req.points as f64).bind(user_id)
        .execute(&mut *tx).await.map_err(AppError::Database)?;

    // Queue the on-chain payout for the batch minter (kind='conversion').
    let payout_id: Uuid = sqlx::query_scalar(
        "INSERT INTO token_rewards (user_id, action, amount, status, kind)
         VALUES ($1, 'conversion', $2, 'pending', 'conversion') RETURNING id"
    )
    .bind(user_id).bind(req.points)
    .fetch_one(&mut *tx).await.map_err(AppError::Database)?;

    // Ledger: points debited for a one-way conversion to on-chain YEET.
    {
        use crate::services::ledger::{self, NewEntry, tx_type, asset};
        ledger::record_in_tx(&mut tx, NewEntry {
            tx_type: tx_type::POINTS_CONVERSION.into(), asset: asset::POINTS.into(),
            amount: -(req.points as f64), fee_amount: 0.0,
            user_id: Some(user_id), user_wallet: Some(wallet.clone()),
            reference_type: Some("payout".into()), reference_id: Some(payout_id.to_string()),
            description: Some(format!("convert {} points to YEET (payout to {})", req.points, wallet)),
            ..Default::default()
        }).await?;
    }

    tx.commit().await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(ConvertResponse {
        converted_points: req.points,
        payout_id,
        wallet_address: wallet,
        points_balance: balance - req.points as f64,
        status: "queued",
    })))
}
