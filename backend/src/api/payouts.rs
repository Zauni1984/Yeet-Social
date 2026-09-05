//! Admin manual approval of point→YEET payouts (conversions).
//!
//! For launch, every user-requested conversion is queued as
//! `status='awaiting_approval'` and does NOT reach the on-chain batch minter
//! until an admin approves it (→ `status='pending'`, which the minter picks up).
//! Rejecting refunds the points to the user. This human gate is deliberate for
//! the first phase; automated rules will replace it once we have real-user data.
use axum::{extract::{Path, Query, State}, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::admin_mod::{check_admin_secret, record_action};

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub secret: String,
    /// 'awaiting_approval' (default) | 'pending' | 'minted' | 'failed' | 'rejected' | 'all'
    pub status: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PayoutRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub username: Option<String>,
    pub wallet_address: Option<String>,
    pub amount: f64,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// How often the on-chain mint has been attempted (see batch_rewards).
    pub mint_attempts: i32,
    /// Last mint error, if any — why a payout is 'failed' or still retrying.
    pub last_error: Option<String>,
}

const SELECT: &str =
    "SELECT r.id, r.user_id, u.username, u.wallet_address, r.amount::float8 AS amount, r.status, r.created_at,
            r.mint_attempts, r.last_error
       FROM token_rewards r JOIN users u ON u.id = r.user_id
      WHERE r.kind = 'conversion'";

/// GET /api/v1/admin/payouts?status=awaiting_approval — the approval queue.
pub async fn admin_list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<ApiResponse<Vec<PayoutRow>>>> {
    check_admin_secret(&q.secret)?;
    let status = q.status.unwrap_or_else(|| "awaiting_approval".into());
    if !["awaiting_approval", "pending", "minted", "failed", "rejected", "all"].contains(&status.as_str()) {
        return Err(AppError::Validation("invalid status filter".into()));
    }
    let rows: Vec<PayoutRow> = if status == "all" {
        sqlx::query_as::<_, PayoutRow>(&format!("{SELECT} ORDER BY r.created_at DESC LIMIT 500"))
            .fetch_all(state.db.pool()).await.map_err(AppError::Database)?
    } else {
        sqlx::query_as::<_, PayoutRow>(&format!("{SELECT} AND r.status = $1 ORDER BY r.created_at DESC LIMIT 500"))
            .bind(&status)
            .fetch_all(state.db.pool()).await.map_err(AppError::Database)?
    };
    Ok(Json(ApiResponse::ok(rows)))
}

#[derive(Debug, Deserialize)]
pub struct DecideRequest {
    pub secret: String,
    pub note: Option<String>,
}

/// POST /api/v1/admin/payouts/:id/approve — release the payout to the batch
/// minter. Only an `awaiting_approval` conversion can be approved.
pub async fn admin_approve(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<DecideRequest>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    check_admin_secret(&req.secret)?;

    let res = sqlx::query(
        "UPDATE token_rewards SET status = 'pending'
          WHERE id = $1 AND kind = 'conversion' AND status = 'awaiting_approval'"
    )
    .bind(id)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;
    if res.rows_affected() == 0 {
        return Err(AppError::Validation("payout not found or not awaiting approval".into()));
    }

    let (uid, uname): (Option<Uuid>, Option<String>) = sqlx::query_as(
        "SELECT r.user_id, u.username FROM token_rewards r JOIN users u ON u.id = r.user_id WHERE r.id = $1"
    )
    .bind(id)
    .fetch_optional(state.db.pool()).await.ok().flatten().unwrap_or((None, None));
    let reason = match req.note.as_deref() {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => format!("payout {id} approved"),
    };
    record_action(
        state.db.pool(), uid, uname.as_deref(),
        "payout_approve", None, Some(reason.as_str()),
        None, None,
    ).await;

    Ok(Json(ApiResponse::ok("approved")))
}

/// POST /api/v1/admin/payouts/:id/reject — refund the points and close the
/// request. Allowed for `awaiting_approval` (not yet released) and `failed`
/// (on-chain mint gave up after max attempts) conversions.
pub async fn admin_reject(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<DecideRequest>,
) -> AppResult<Json<ApiResponse<&'static str>>> {
    check_admin_secret(&req.secret)?;

    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;

    // Lock the row and confirm it is refundable (never a 'pending' one — the
    // minter may be paying it out right now — and never a 'minted' one); grab
    // the amount + user so we can refund exactly and atomically.
    let row: Option<(Uuid, f64)> = sqlx::query_as(
        "SELECT user_id, amount::float8 FROM token_rewards
          WHERE id = $1 AND kind = 'conversion' AND status IN ('awaiting_approval', 'failed') FOR UPDATE"
    )
    .bind(id)
    .fetch_optional(&mut *tx).await.map_err(AppError::Database)?;
    let (user_id, amount) = row.ok_or_else(||
        AppError::Validation("payout not found or not refundable (must be awaiting_approval or failed)".into()))?;

    sqlx::query("UPDATE token_rewards SET status = 'rejected' WHERE id = $1")
        .bind(id).execute(&mut *tx).await.map_err(AppError::Database)?;

    // Refund the debited points back to the user.
    sqlx::query("UPDATE users SET yeet_token_balance = COALESCE(yeet_token_balance, 0) + $1 WHERE id = $2")
        .bind(amount).bind(user_id)
        .execute(&mut *tx).await.map_err(AppError::Database)?;

    crate::services::ledger::record_in_tx(&mut tx, crate::services::ledger::NewEntry {
        tx_type: crate::services::ledger::tx_type::PAYOUT_REFUND.into(),
        asset: crate::services::ledger::asset::POINTS.into(),
        amount, // positive: credited back to the user
        user_id: Some(user_id),
        reference_type: Some("payout".into()),
        reference_id: Some(id.to_string()),
        description: Some(format!("payout {id} rejected by admin — {amount} points refunded")),
        ..Default::default()
    }).await?;

    tx.commit().await.map_err(AppError::Database)?;

    let uname: Option<String> = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(user_id).fetch_optional(state.db.pool()).await.ok().flatten();
    record_action(
        state.db.pool(), Some(user_id), uname.as_deref(),
        "payout_reject", None, req.note.as_deref(),
        None, None,
    ).await;

    Ok(Json(ApiResponse::ok("rejected")))
}
