#![allow(dead_code)]
//! YEET token reward service with daily cap.
use uuid::Uuid;
use crate::{db::Database, error::AppResult, AppError};

pub mod rewards {
    // Defaults. The posting reward and the per-user daily cap are tunable at
    // runtime via env (see the accessors below) so the tokenomics can be
    // adjusted without a redeploy.
    pub const POST_CREATED: i64 = 10;   // 10 points per qualifying article
    pub const POST_LIKED: i64 = 1;
    pub const POST_RESHARED: i64 = 2;
    pub const COMMENT_POSTED: i64 = 1;
    pub const DAILY_LOGIN: i64 = 2;
    pub const NFT_MINTED: i64 = 10;
    pub const DAILY_CAP: i64 = 1000;    // max reward points a user can earn per day
    /// Minimum article length (characters, trimmed) that earns the posting reward.
    pub const POST_MIN_CHARS: usize = 120;
}

/// One-time signup bonus parameters.
pub mod registration {
    pub const BONUS_POINTS: i64 = 1000;      // points granted once, per identity
    pub const MAX_RECIPIENTS: i64 = 100_000; // only the first N eligible users
}

/// Read a non-negative i64 from the environment, falling back to `default`.
fn env_i64(key: &str, default: i64) -> i64 {
    std::env::var(key).ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .filter(|n| *n >= 0)
        .unwrap_or(default)
}

/// Per-user daily reward cap (points). Override: `YEET_DAILY_POINTS_CAP`.
pub fn daily_cap() -> i64 { env_i64("YEET_DAILY_POINTS_CAP", rewards::DAILY_CAP) }
/// Points awarded for a qualifying article. Override: `YEET_POST_REWARD`.
pub fn post_reward() -> i64 { env_i64("YEET_POST_REWARD", rewards::POST_CREATED) }
/// Minimum trimmed character count that earns the posting reward.
/// Override: `YEET_POST_MIN_CHARS`.
pub fn post_min_chars() -> usize { env_i64("YEET_POST_MIN_CHARS", rewards::POST_MIN_CHARS as i64) as usize }
/// Signup-bonus size in points. Override: `YEET_REGISTRATION_BONUS`.
pub fn registration_bonus_points() -> i64 { env_i64("YEET_REGISTRATION_BONUS", registration::BONUS_POINTS) }
/// How many users may receive the signup bonus. Override: `YEET_REGISTRATION_BONUS_MAX`.
pub fn registration_bonus_max() -> i64 { env_i64("YEET_REGISTRATION_BONUS_MAX", registration::MAX_RECIPIENTS) }

#[derive(Debug, Clone, sqlx::Type, serde::Serialize, serde::Deserialize)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum RewardAction {
    PostCreated, PostLiked, PostReshared, CommentPosted, DailyLogin, NftMinted, TipReceived,
}

/// Grant engagement POINTS (docs/mica/05). Rewards are now credited to the
/// off-chain points ledger (`users.yeet_token_balance`) and are NOT auto-minted
/// on-chain — a user turns points into YEET only via the explicit one-way
/// conversion (see api::points::convert). An audit row is kept in
/// token_rewards with kind='reward' and a terminal status so the batch minter
/// (which now only processes kind='conversion') never pays it out.
pub async fn grant_reward(db: &Database, user_id: Uuid, action: RewardAction, amount: i64) -> AppResult<i64> {
    // Daily cap counts today's granted reward points, regardless of status.
    let today_total: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount), 0)::bigint FROM token_rewards
         WHERE user_id = $1 AND created_at >= CURRENT_DATE AND kind = 'reward'"
    )
    .bind(user_id).fetch_one(db.pool()).await.map_err(AppError::Database)?;

    let remaining = daily_cap() - today_total;
    if remaining <= 0 { return Ok(0); }
    let actual = amount.min(remaining);
    let action_str = format!("{:?}", action).to_lowercase().replace(" ", "_");

    let mut tx = db.pool().begin().await.map_err(AppError::Database)?;
    // Audit row (never minted: kind='reward', status='rewarded').
    sqlx::query(
        "INSERT INTO token_rewards (user_id, action, amount, status, kind)
         VALUES ($1, $2, $3, 'rewarded', 'reward')"
    )
    .bind(user_id).bind(&action_str).bind(actual)
    .execute(&mut *tx).await.map_err(AppError::Database)?;
    // Credit spendable points.
    sqlx::query(
        "UPDATE users SET yeet_token_balance = COALESCE(yeet_token_balance, 0) + $1 WHERE id = $2"
    )
    .bind(actual as f64).bind(user_id)
    .execute(&mut *tx).await.map_err(AppError::Database)?;

    // Ledger: engagement points earned.
    crate::services::ledger::record_in_tx(&mut tx, crate::services::ledger::NewEntry {
        tx_type: crate::services::ledger::tx_type::REWARD_GRANT.into(),
        asset: crate::services::ledger::asset::POINTS.into(),
        amount: actual as f64,
        user_id: Some(user_id),
        reference_type: Some("reward".into()),
        reference_id: Some(action_str.clone()),
        description: Some(format!("reward: {action_str}")),
        ..Default::default()
    }).await?;

    tx.commit().await.map_err(AppError::Database)?;
    Ok(actual)
}

/// Advisory-lock key serialising the "count recipients, then grant" step so the
/// first-N signup-bonus cap can never be exceeded under concurrency.
const REG_BONUS_LOCK_KEY: i64 = 0x59_45_45_54_52_45_47; // "YEETREG"

/// Grant the one-time signup bonus once a user has completed BOTH email
/// double-opt-in (`email_verified_at`) AND KYC/age verification
/// (`age_verified_at`), for the first `registration_bonus_max()` eligible users.
///
/// Safe to call from either completion hook (email-verify or KYC-approve),
/// whichever finishes last: it is idempotent (a user with a non-NULL
/// `registration_bonus_granted_at` is skipped) and concurrency-safe (the
/// recipient count and the grant happen under one advisory lock + row lock).
/// Returns the points granted (0 if not eligible / already paid / cap reached).
pub async fn maybe_grant_registration_bonus(db: &Database, user_id: Uuid) -> AppResult<i64> {
    let bonus = registration_bonus_points();
    let max_recipients = registration_bonus_max();
    if bonus <= 0 || max_recipients <= 0 { return Ok(0); }

    let mut tx = db.pool().begin().await.map_err(AppError::Database)?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(REG_BONUS_LOCK_KEY)
        .execute(&mut *tx).await.map_err(AppError::Database)?;

    type Ts = Option<chrono::DateTime<chrono::Utc>>;
    let row: Option<(Ts, Ts, Ts)> = sqlx::query_as(
        "SELECT email_verified_at, age_verified_at, registration_bonus_granted_at
           FROM users WHERE id = $1 FOR UPDATE"
    )
    .bind(user_id)
    .fetch_optional(&mut *tx).await.map_err(AppError::Database)?;

    let (email_v, age_v, granted) = match row {
        Some(r) => r,
        None => return Ok(0), // user vanished; tx rolls back on drop
    };
    if granted.is_some() { return Ok(0); }             // already paid — idempotent
    if email_v.is_none() || age_v.is_none() { return Ok(0); } // both gates required

    let recipients: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM users WHERE registration_bonus_granted_at IS NOT NULL"
    )
    .fetch_one(&mut *tx).await.map_err(AppError::Database)?;
    if recipients >= max_recipients { return Ok(0); }  // first-N window closed

    // Stamp + credit points atomically.
    sqlx::query(
        "UPDATE users SET registration_bonus_granted_at = NOW(),
             yeet_token_balance = COALESCE(yeet_token_balance, 0) + $1
           WHERE id = $2"
    )
    .bind(bonus as f64).bind(user_id)
    .execute(&mut *tx).await.map_err(AppError::Database)?;

    // Audit row (kind='reward' → never picked up by the on-chain batch minter).
    sqlx::query(
        "INSERT INTO token_rewards (user_id, action, amount, status, kind)
         VALUES ($1, 'registration_bonus', $2, 'rewarded', 'reward')"
    )
    .bind(user_id).bind(bonus)
    .execute(&mut *tx).await.map_err(AppError::Database)?;

    crate::services::ledger::record_in_tx(&mut tx, crate::services::ledger::NewEntry {
        tx_type: crate::services::ledger::tx_type::REGISTRATION_BONUS.into(),
        asset: crate::services::ledger::asset::POINTS.into(),
        amount: bonus as f64,
        user_id: Some(user_id),
        reference_type: Some("registration_bonus".into()),
        description: Some(format!("registration bonus: {bonus} points")),
        ..Default::default()
    }).await?;

    tx.commit().await.map_err(AppError::Database)?;
    Ok(bonus)
}

/// Points currently queued for on-chain payout (kind='conversion', not yet
/// minted). Renamed conceptually from "pending rewards" — engagement rewards
/// are no longer pending mints.
pub async fn get_pending_payout(db: &Database, user_id: Uuid) -> AppResult<i64> {
    let b: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount), 0)::bigint FROM token_rewards
         WHERE user_id = $1 AND kind = 'conversion' AND status = 'pending' AND tx_hash IS NULL"
    )
    .bind(user_id).fetch_one(db.pool()).await.map_err(AppError::Database)?;
    Ok(b)
}
