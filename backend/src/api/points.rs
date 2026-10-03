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
/// Advisory-lock key serialising pool-check + queue-insert across requests.
const CONVERT_LOCK_KEY: i64 = 0x59_45_45_54_43_4e_56; // "YEETCNV"

#[derive(Debug, Deserialize)]
pub struct ConvertRequest {
    /// Whole points to convert to YEET at the current `conversion_rates` rate. Integer.
    pub points: i64,
}

#[derive(Debug, Serialize)]
pub struct ConvertResponse {
    pub converted_points: i64,
    /// YEET that will be minted (= points × rate).
    pub yeet_amount: f64,
    /// YEET per point applied to this request.
    pub rate: f64,
    pub payout_id: Uuid,
    pub wallet_address: String,
    /// Remaining spendable points after the debit.
    pub points_balance: f64,
    pub status: &'static str,
}

/// Public view of the conversion rate: what applies now and what has been
/// announced (Terms §6: changes are prospective and announced in advance).
#[derive(Debug, Serialize)]
pub struct RateResponse {
    pub current: crate::services::tokens::ConversionRate,
    pub upcoming: Vec<crate::services::tokens::ConversionRate>,
    /// Minimum announcement lead time in days for any change.
    pub notice_days: i64,
}

/// GET /api/v1/points/rate — public, no auth.
pub async fn get_rate(State(state): State<AppState>) -> AppResult<Json<ApiResponse<RateResponse>>> {
    Ok(Json(ApiResponse::ok(RateResponse {
        current: crate::services::tokens::current_conversion_rate(&state.db).await?,
        upcoming: crate::services::tokens::upcoming_conversion_rates(&state.db).await?,
        notice_days: crate::services::tokens::rate_notice_days(),
    })))
}

#[derive(Debug, Deserialize)]
pub struct ScheduleRateRequest {
    pub secret: String,
    /// YEET per point from `valid_from` on.
    pub rate: f64,
    pub valid_from: chrono::DateTime<chrono::Utc>,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AdminRateQuery { pub secret: String }

/// POST /api/v1/admin/conversion-rate — announce a future rate. Refused when
/// it would apply sooner than YEET_RATE_NOTICE_DAYS. Remember to publish the
/// change to users as well (changelog bot entry) — this endpoint only makes
/// it visible via GET /points/rate and the conversion dialog.
pub async fn admin_schedule_rate(
    State(state): State<AppState>,
    Json(req): Json<ScheduleRateRequest>,
) -> AppResult<Json<ApiResponse<crate::services::tokens::ConversionRate>>> {
    crate::api::admin_mod::check_admin_secret(&req.secret)?;
    let row = crate::services::tokens::schedule_conversion_rate(
        &state.db, req.rate, req.valid_from, req.note.as_deref(), "admin",
    ).await?;
    crate::api::admin_mod::record_action(
        state.db.pool(), None, None, "conversion_rate_scheduled", None,
        Some(&format!("1 point = {} YEET from {}{}", req.rate, req.valid_from.to_rfc3339(),
            req.note.as_deref().map(|n| format!(" — {n}")).unwrap_or_default())),
        None, None,
    ).await;
    Ok(Json(ApiResponse::ok(row)))
}

/// GET /api/v1/admin/conversion-rate?secret= — full history, newest first.
pub async fn admin_rate_history(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<AdminRateQuery>,
) -> AppResult<Json<ApiResponse<Vec<crate::services::tokens::ConversionRate>>>> {
    crate::api::admin_mod::check_admin_secret(&q.secret)?;
    Ok(Json(ApiResponse::ok(crate::services::tokens::conversion_rate_history(&state.db).await?)))
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

    // Serialise conversions: the pool check and the queue insert below must not
    // interleave across concurrent requests, or two callers could both pass the
    // check and jointly overshoot the pool. Every earlier conversion commits
    // (and thus is visible to pool_status) before this lock is granted.
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(CONVERT_LOCK_KEY)
        .execute(&mut *tx).await.map_err(AppError::Database)?;

    // Rate in force at this moment (versioned, prospective changes only —
    // docs/mica/09 D4). YEET to mint = points × rate, 8-decimal precision.
    let rate = crate::services::tokens::current_conversion_rate(&state.db).await?;
    let yeet_amount = ((req.points as f64) * rate.rate * 1e8).round() / 1e8;
    if yeet_amount <= 0.0 {
        return Err(AppError::Validation("Conversion amount rounds to zero at the current rate".into()));
    }

    // Drain guard: never queue a payout the finite on-chain conversion pool
    // cannot cover. The pool refills from platform fees, so this is a
    // self-healing ceiling rather than a hard stop.
    let pool = crate::services::tokens::pool_status(&state.db).await?;
    if yeet_amount > pool.remaining {
        return Err(AppError::Forbidden("CONVERSION_POOL_EXHAUSTED".into()));
    }

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

    // F6 — refuse up front rather than debiting points for a payout the
    // batch minter would park as failed anyway (screened again at mint time).
    if crate::services::sanctions::check(&wallet) == crate::services::sanctions::Verdict::Sanctioned {
        return Err(AppError::Forbidden("SANCTIONED_ADDRESS".into()));
    }

    if balance < req.points as f64 {
        return Err(AppError::Validation("Insufficient points".into()));
    }

    // Debit points (one-way).
    sqlx::query("UPDATE users SET yeet_token_balance = yeet_token_balance - $1 WHERE id = $2")
        .bind(req.points as f64).bind(user_id)
        .execute(&mut *tx).await.map_err(AppError::Database)?;

    // Queue the payout for MANUAL ADMIN APPROVAL (kind='conversion',
    // status='awaiting_approval'). The on-chain batch minter only ever picks up
    // status='pending', so nothing is paid out until an admin approves it (which
    // flips the row to 'pending'). Rejecting refunds the points. This human gate
    // is deliberate for launch; automated rules will replace it later.
    let payout_id: Uuid = sqlx::query_scalar(
        "INSERT INTO token_rewards (user_id, action, amount, status, kind, points_debited, rate, wallet_address)
         VALUES ($1, 'conversion', $2, 'awaiting_approval', 'conversion', $3, $4, $5) RETURNING id"
    )
    .bind(user_id).bind(yeet_amount).bind(req.points as f64).bind(rate.rate).bind(&wallet)
    .fetch_one(&mut *tx).await.map_err(AppError::Database)?;

    // Ledger: points debited for a one-way conversion to on-chain YEET.
    {
        use crate::services::ledger::{self, NewEntry, tx_type, asset};
        ledger::record_in_tx(&mut tx, NewEntry {
            tx_type: tx_type::POINTS_CONVERSION.into(), asset: asset::POINTS.into(),
            amount: -(req.points as f64), fee_amount: 0.0,
            user_id: Some(user_id), user_wallet: Some(wallet.clone()),
            reference_type: Some("payout".into()), reference_id: Some(payout_id.to_string()),
            description: Some(format!("convert {} points → {yeet_amount} YEET at 1 point = {} YEET (payout to {})", req.points, rate.rate, wallet)),
            ..Default::default()
        }).await?;
    }

    tx.commit().await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(ConvertResponse {
        converted_points: req.points,
        yeet_amount,
        rate: rate.rate,
        payout_id,
        wallet_address: wallet,
        points_balance: balance - req.points as f64,
        status: "awaiting_approval",
    })))
}
