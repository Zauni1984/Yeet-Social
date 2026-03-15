use axum::{extract::State, Json};
use crate::AppState;
use shared::ApiResponse;
use serde::Serialize;
use uuid::Uuid;

#[derive(Serialize)]
pub struct TokenBalance {
    pub yeet_db: f64,      // off-chain balance (fast)
    pub yeet_bsc: f64,     // on-chain BEP-20 balance
    pub pending_rewards: f64,
}

pub async fn get_balance(
    State(state): State<AppState>,
) -> Json<ApiResponse<TokenBalance>> {
    let user_id = Uuid::new_v4(); // TODO: from JWT

    let row = sqlx::query!(
        "SELECT yeet_token_balance, wallet_address FROM users WHERE id = $1",
        user_id
    )
    .fetch_optional(&state.db.pool)
    .await;

    match row {
        Ok(Some(r)) => {
            let bsc_balance = if let Some(wallet) = r.wallet_address {
                state.bsc.get_yeet_balance(&wallet).await.unwrap_or(0.0)
            } else { 0.0 };

            Json(ApiResponse::ok(TokenBalance {
                yeet_db: r.yeet_token_balance.unwrap_or(0.0),
                yeet_bsc: bsc_balance,
                pending_rewards: 0.0,
            }))
        }
        _ => Json(ApiResponse::err("User not found")),
    }
}

pub async fn get_rewards(
    State(state): State<AppState>,
) -> Json<ApiResponse<Vec<shared::TokenReward>>> {
    let user_id = Uuid::new_v4(); // TODO: from JWT

    let rows = sqlx::query!(
        r#"
        SELECT id, user_id, action::text, amount, tx_hash, created_at
        FROM token_rewards
        WHERE user_id = $1
        ORDER BY created_at DESC
        LIMIT 50
        "#,
        user_id
    )
    .fetch_all(&state.db.pool)
    .await;

    match rows {
        Ok(rows) => {
            let rewards = rows.into_iter().map(|r| shared::TokenReward {
                id: r.id,
                user_id: r.user_id,
                action: serde_json::from_str(
                    &format!("\"{}\"", r.action.unwrap_or_default())
                ).unwrap_or(shared::RewardAction::DailyLogin),
                amount: r.amount.unwrap_or(0.0),
                tx_hash: r.tx_hash,
                created_at: r.created_at,
            }).collect();
            Json(ApiResponse::ok(rewards))
        }
        Err(_) => Json(ApiResponse::err("Query failed")),
    }
}
