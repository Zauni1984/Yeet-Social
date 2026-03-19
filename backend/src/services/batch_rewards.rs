use anyhow::Result;
use ethers::{
    prelude::*,
    providers::{Http, Provider},
    signers::LocalWallet,
};
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{info, error, warn};

use crate::AppState;

/// Batch reward job: runs every hour, collects pending off-chain
/// rewards from DB and submits a single batchMintRewards tx to BSC.
/// This keeps gas costs minimal (1 tx per hour instead of per action).
pub async fn start_reward_batch_job(state: AppState) {
    let privkey = match std::env::var("REWARDS_MINTER_PRIVKEY") {
        Ok(k) if !k.is_empty() => k,
        _ => {
            warn!("REWARDS_MINTER_PRIVKEY not set â batch reward job disabled");
            return;
        }
    };

    let mut ticker = interval(Duration::from_secs(3600)); // every hour

    loop {
        ticker.tick().await;
        info!("Running batch reward mint job...");

        if let Err(e) = run_batch(&state, &privkey).await {
            error!("Batch reward job failed: {e}");
        }
    }
}

async fn run_batch(state: &AppState, privkey: &str) -> Result<()> {
    // Fetch all unminted rewards (tx_hash IS NULL) from DB
    #[allow(dead_code)]
    struct RewardRow {
        id: uuid::Uuid,
        wallet_address: Option<String>,
        action: Option<String>,
        amount: Option<f64>,
    }
    let rows: Vec<RewardRow> = sqlx::query_as!(
        RewardRow,
        r#"SELECT r.id as "id: uuid::Uuid", u.wallet_address, r.action::text as action, r.amount::float8 as amount
        FROM token_rewards r JOIN users u ON u.id = r.user_id
        WHERE r.tx_hash IS NULL AND u.wallet_address IS NOT NULL
        ORDER BY r.created_at ASC LIMIT 500"#
    )
    .fetch_all(&state.db.pool)
    .await?;

    if rows.is_empty() {
        info!("No pending rewards to mint.");
        return Ok(());
    }

    info!("Minting {} reward records on BSC...", rows.len());

    // Build arrays for batchMintRewards(address[], uint256[], string[])
    let recipients: Vec<Address> = rows.iter()
        .filter_map(|r| r.wallet_address.as_ref())
        .map(|w| w.parse::<Address>())
        .collect::<Result<Vec<ethers::types::Address>, _>>().map_err(|e| anyhow::anyhow!(e))?;

    let amounts: Vec<U256> = rows.iter()
        .map(|r| {
            let yeet: f64 = r.amount.unwrap_or(0.0);
            let wei = (yeet * 1e18) as u128;
            U256::from(wei)
        })
        .collect::<Vec<_>>();

    let actions: Vec<String> = rows.iter()
        .map(|r| r.action.clone().unwrap_or_default())
        .collect::<Vec<String>>();

    // Set up signer
    let wallet: LocalWallet = privkey.parse::<LocalWallet>()?
        .with_chain_id(56u64); // BSC mainnet chain ID

    let provider = Provider::<Http>::try_from(
        std::env::var("BSC_RPC_URL")
            .unwrap_or_else(|_| "https://bsc-dataseed.binance.org/".into())
    )?;
    let client = Arc::new(SignerMiddleware::new(provider, wallet));

    // Call batchMintRewards on YeetToken contract
    let token_addr: Address = std::env::var("YEET_TOKEN_ADDRESS")?.parse()?;

    abigen!(
    YeetToken,
    r#"[{"inputs":[{"name":"recipients","type":"address[]"},{"name":"amounts","type":"uint256[]"},{"name":"actions","type":"string[]"}],"name":"batchMintRewards","outputs":[],"stateMutability":"nonpayable","type":"function"}]"#
);

    let contract = YeetToken::new(token_addr, client);
    let tx = contract
        .batch_mint_rewards(recipients, amounts, actions)
        .gas(500_000u64)
        .send()
        .await?
        .await?
        .ok_or_else(|| anyhow::anyhow!("No receipt"))?;

    let tx_hash = format!("{:?}", tx.transaction_hash);
    info!("Batch mint tx: {}", tx_hash);

    // Mark all rewarded rows with the tx hash
    let ids: Vec<uuid::Uuid> = rows.iter().map(|r| r.id).collect::<Vec<_>>();
    sqlx::query!(
        "UPDATE token_rewards SET tx_hash = $1 WHERE id = ANY($2)",
        tx_hash,
        &ids,
    )
    .execute(&state.db.pool)
    .await?;

    info!("Marked {} rewards as minted.", ids.len());
    Ok(())
}

/// Also runs a cleanup job to soft-delete expired non-NFT posts
pub async fn start_cleanup_job(state: AppState) {
    let mut ticker = interval(Duration::from_secs(300)); // every 5 minutes
    loop {
        ticker.tick().await;
        match sqlx::query_scalar!(
            r#"SELECT cleanup_expired_posts() as "count!""#
        )
        .fetch_one(&state.db.pool)
        .await
        {
            Ok(n) if n > 0 => info!("Cleaned up {} expired posts.", n),
            Ok(_) => {}
            Err(e) => error!("Cleanup job error: {e}"),
        }
    }
}
