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

pub async fn grant_reward(db: &Database, user_id: Uuid, action: RewardAction, amount: i64) -> AppResult<i64> {
    let today_total: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount), 0)::bigint FROM token_rewards
         WHERE user_id = $1 AND created_at >= CURRENT_DATE AND status = 'pending'"
    )
    .bind(user_id).fetch_one(db.pool()).await.map_err(AppError::Database)?;

    let remaining = rewards::DAILY_CAP - today_total;
    if remaining <= 0 { return Ok(0); }
    let actual = amount.min(remaining);
    let action_str = format!("{:?}", action).to_lowercase().replace(" ", "_");

    sqlx::query(
        "INSERT INTO token_rewards (user_id, action, amount, status) VALUES ($1, $2, $3, 'pending')"
    )
    .bind(user_id).bind(&action_str).bind(actual)
    .execute(db.pool()).await.map_err(AppError::Database)?;
    Ok(actual)
}

pub async fn get_pending_balance(db: &Database, user_id: Uuid) -> AppResult<i64> {
    let b: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount), 0)::bigint FROM token_rewards WHERE user_id = $1 AND status = 'pending'"
    )
    .bind(user_id).fetch_one(db.pool()).await.map_err(AppError::Database)?;
    Ok(b)
}
