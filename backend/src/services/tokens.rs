#![allow(dead_code)]
//! YEET token reward service with daily cap.
use uuid::Uuid;
use crate::{db::Database, error::AppResult, AppError};

pub mod rewards {
    pub const POST_CREATED: i64 = 5;
    pub const POST_LIKED: i64 = 1;
    pub const POST_RESHARED: i64 = 2;
    pub const COMMENT_POSTED: i64 = 1;
    pub const DAILY_LOGIN: i64 = 2;
    pub const NFT_MINTED: i64 = 10;
    pub const DAILY_CAP: i64 = 50;
}

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

    let remaining = rewards::DAILY_CAP - today_total;
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
