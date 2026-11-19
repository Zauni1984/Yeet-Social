//! Token balance + rewards handlers.
use axum::{extract::State, Json};
use serde::Serialize;
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, models::ApiResponse, services::tokens};
use crate::api::middleware::AuthUser;

#[derive(Debug, Serialize)]
pub struct BalanceResponse {
    /// Spendable off-chain POINTS (users.yeet_token_balance). Earned only;
    /// spent on in-app tips/PPV; convertible one-way to on-chain YEET.
    pub points: f64,
    /// Points queued for on-chain payout (a pending points→YEET conversion).
    pub pending_payout: i64,
    /// The user's linked EXTERNAL wallet (payout target), or null.
    pub wallet_address: Option<String>,
    // ── Back-compat aliases (older clients read these) ──────────────────
    /// Deprecated alias of `points`.
    pub yeet_balance: f64,
    /// Deprecated alias of `pending_payout`.
    pub pending_yeet: i64,
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
        "SELECT COALESCE(yeet_token_balance, 0)::float8 AS yeet_balance, wallet_address
           FROM users WHERE id = $1"
    )
    .bind(user_id)
    .fetch_one(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    let pending = tokens::get_pending_payout(&state.db, user_id).await?;

    Ok(Json(ApiResponse::ok(BalanceResponse {
        points: row.0,
        pending_payout: pending,
        wallet_address: row.1.clone(),
        yeet_balance: row.0,
        pending_yeet: pending,
    })))
}

pub async fn get_rewards(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<RewardsResponse>>> {
    let user_id = caller_user_id(&state, &auth).await?;
    // Lifetime reward points come from the reward journal (the former
    // users.total_yeet_earned column never existed in the migrated schema,
    // so this endpoint answered 500 — found while closing audit M-L2).
    let total_earned: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount), 0)::bigint FROM token_rewards WHERE user_id = $1 AND kind = 'reward'"
    )
    .bind(user_id).fetch_one(state.db.pool()).await.map_err(AppError::Database)?;

    // Daily cap counts only granted rewards, never conversions (audit M-L2).
    let today_earned: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount), 0)::bigint FROM token_rewards WHERE user_id = $1 AND created_at >= CURRENT_DATE AND kind = 'reward' AND action <> 'registration_bonus'"
    )
    .bind(user_id).fetch_one(state.db.pool()).await.map_err(AppError::Database)?;

    let cap = tokens::daily_cap();
    Ok(Json(ApiResponse::ok(RewardsResponse {
        total_earned,
        daily_limit: cap,
        daily_remaining: (cap - today_earned).max(0),
    })))
}

/// GET /api/v1/tokens/pool — public conversion-pool health (drain transparency).
/// Aggregate only, no PII; supports the "everything visible on-chain" ethos.
pub async fn get_pool(
    State(state): State<AppState>,
) -> AppResult<Json<ApiResponse<tokens::PoolStatus>>> {
    Ok(Json(ApiResponse::ok(tokens::pool_status(&state.db).await?)))
}

/// Public reward rules as the server applies them right now. The numbers are
/// tunable at runtime (env), so documentation and clients read them here
/// instead of hard-coding them.
#[derive(Debug, Serialize)]
pub struct RewardParams {
    pub post_reward: i64,
    pub post_min_chars: usize,
    pub comment_reward: i64,
    pub reshare_reward: i64,
    pub daily_cap: i64,
    pub registration_bonus: i64,
    pub registration_bonus_max_recipients: i64,
    pub registration_bonus_granted: i64,
    pub registration_bonus_remaining: i64,
    pub tip_min: f64,
    pub platform_fee_pct: f64,
    pub convert_min_points: i64,
    pub convert_requires_verification: bool,
    pub conversion_rate: f64,
    pub rate_notice_days: i64,
    pub taper_threshold_pct: f64,
    pub taper_factor: f64,
    pub tapered: bool,
    pub live_promotion_basic: i64,
    pub live_promotion_boost: i64,
}

/// GET /api/v1/tokens/params — public, aggregate only.
pub async fn get_params(
    State(state): State<AppState>,
) -> AppResult<Json<ApiResponse<RewardParams>>> {
    let granted: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM users WHERE registration_bonus_granted_at IS NOT NULL"
    ).fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
    let max = tokens::registration_bonus_max();
    let pool = tokens::pool_status(&state.db).await?;
    let rate = tokens::current_conversion_rate(&state.db).await?;
    Ok(Json(ApiResponse::ok(RewardParams {
        post_reward: tokens::post_reward(),
        post_min_chars: tokens::post_min_chars(),
        comment_reward: tokens::rewards::COMMENT_POSTED,
        reshare_reward: tokens::rewards::POST_RESHARED,
        daily_cap: tokens::daily_cap(),
        registration_bonus: tokens::registration_bonus_points(),
        registration_bonus_max_recipients: max,
        registration_bonus_granted: granted,
        registration_bonus_remaining: (max - granted).max(0),
        tip_min: crate::api::tips::MIN_TIP_AMOUNT,
        platform_fee_pct: crate::api::tips::PLATFORM_FEE_PCT,
        convert_min_points: crate::api::points::MIN_CONVERT_POINTS,
        convert_requires_verification: true,
        conversion_rate: rate.rate,
        rate_notice_days: tokens::rate_notice_days(),
        taper_threshold_pct: tokens::taper_threshold_pct(),
        taper_factor: tokens::taper_factor(),
        tapered: pool.tapered,
        live_promotion_basic: 10,
        live_promotion_boost: 50,
    })))
}
