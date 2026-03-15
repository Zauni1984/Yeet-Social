use crate::AppState;
use shared::{RewardAction, TokenReward};
use uuid::Uuid;
use anyhow::Result;

/// Award YEET tokens off-chain (DB) for a given user action.
/// A nightly job batches these into on-chain BSC transactions.
pub async fn reward_action(
    state: &AppState,
    user_id: Uuid,
    action: RewardAction,
) -> Result<TokenReward> {
    let amount = action.reward_amount();
    let action_str = serde_json::to_string(&action)?
        .trim_matches('"')
        .to_string();

    // Prevent duplicate daily-login rewards within 24h
    if action == RewardAction::DailyLogin {
        let already = sqlx::query_scalar!(
            r#"SELECT EXISTS(
                SELECT 1 FROM token_rewards
                WHERE user_id = $1
                  AND action = 'daily_login'::reward_action
                  AND created_at > NOW() - INTERVAL '24 hours'
            ) as "exists!" "#,
            user_id
        )
        .fetch_one(&state.db.pool)
        .await?;

        if already {
            return Err(anyhow::anyhow!("Already rewarded today"));
        }
    }

    let row = sqlx::query!(
        r#"INSERT INTO token_rewards (id, user_id, action, amount, created_at)
        VALUES ($1, $2, $3::reward_action, $4, NOW())
        RETURNING id, created_at"#,
        Uuid::new_v4(),
        user_id,
        action_str,
        amount,
    )
    .fetch_one(&state.db.pool)
    .await?;

    // Update user off-chain balance (fast path)
    sqlx::query!(
        "UPDATE users SET yeet_token_balance = yeet_token_balance + $1 WHERE id = $2",
        amount,
        user_id
    )
    .execute(&state.db.pool)
    .await?;

    Ok(TokenReward {
        id: row.id,
        user_id,
        action,
        amount,
        tx_hash: None,
        created_at: row.created_at,
    })
}
