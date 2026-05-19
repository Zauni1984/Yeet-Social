//! YEET Credit cashout worker.
//!
//! Drains the `credit_withdrawals` queue once per [`POLL_INTERVAL`].
//! Each tick has two phases:
//!
//! 1. **Submit** — fetch up to [`MAX_SUBMITS_PER_TICK`] `pending` rows,
//!    JOIN to `users` for the recipient's `wallet_address`, sign +
//!    broadcast a BEP-20 `transfer` from the custodian, advance the
//!    row to `submitted` with the resulting `tx_hash`.
//! 2. **Confirm** — fetch up to [`MAX_CONFIRMS_PER_TICK`] `submitted`
//!    rows, look up the receipt, advance to `confirmed` (status = 1)
//!    or `failed` (status = 0, refunds credit atomically).
//!
//! Transient RPC errors are logged and left for the next tick to
//! retry; the worker never panics. The signer plumbing mirrors
//! `services::batch_rewards`, but uses a different env var
//! (`CUSTODIAN_PRIVKEY`) so a leak is scoped to one role.

use std::sync::Arc;

use anyhow::Result;
use ethers::prelude::*;
use ethers::signers::LocalWallet;
use sqlx::PgPool;
use tokio::time::{Duration, interval};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::AppState;

abigen!(
    YeetToken,
    r#"[
        function transfer(address to, uint256 amount) returns (bool)
    ]"#
);

/// How often the worker wakes up to look for work.
///
/// Hourly matches the user expectation set by similar services
/// (`batch_rewards`); finer cadence would just hammer the RPC for
/// minimal UX gain at this scale.
const POLL_INTERVAL: Duration = Duration::from_secs(3600);

/// Maximum rows broadcast per submit phase.
///
/// Bounded so a queue spike doesn't fan out to thousands of
/// simultaneous RPC calls. The worker catches up across multiple
/// ticks if there's a backlog.
const MAX_SUBMITS_PER_TICK: i64 = 50;

/// Maximum rows whose receipts we poll per confirm phase.
const MAX_CONFIRMS_PER_TICK: i64 = 200;

/// Fallback BSC RPC endpoint when `BSC_RPC_URL` is not set.
const DEFAULT_BSC_RPC: &str = "https://bsc-dataseed.bnbchain.org";

/// Fallback YEET token address when `YEET_TOKEN_ADDRESS` is not set.
const DEFAULT_YEET_TOKEN_ADDRESS: &str = "0x0f1963829c6cc7A925E2F46949C3b248D69297c7";

/// BSC mainnet chain id, used when `BSC_CHAIN_ID` is not overridden.
///
/// Operators on testnet should set `BSC_CHAIN_ID=97`.
const DEFAULT_BSC_CHAIN_ID: u64 = 56;

/// Spawn-and-loop entry point. Called from `main.rs` as a background
/// `tokio::spawn` task.
///
/// Bails (logging) when any required config is missing or malformed.
/// Once the loop starts, transient errors are absorbed and retried.
pub async fn start_credit_payout_job(state: AppState) {
    let privkey = match std::env::var("CUSTODIAN_PRIVKEY") {
        Ok(k) if !k.is_empty() => k,
        _ => {
            warn!("CUSTODIAN_PRIVKEY not set — credit payout job disabled");
            return;
        }
    };

    let rpc_url = std::env::var("BSC_RPC_URL").unwrap_or_else(|_| DEFAULT_BSC_RPC.into());
    let chain_id = std::env::var("BSC_CHAIN_ID")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_BSC_CHAIN_ID);
    let token_addr_str =
        std::env::var("YEET_TOKEN_ADDRESS").unwrap_or_else(|_| DEFAULT_YEET_TOKEN_ADDRESS.into());

    let token_addr: Address = match token_addr_str.parse() {
        Ok(a) => a,
        Err(e) => {
            error!(error = %e, addr = %token_addr_str, "invalid YEET_TOKEN_ADDRESS, credit payout disabled");
            return;
        }
    };

    let wallet: LocalWallet = match privkey.parse::<LocalWallet>() {
        Ok(w) => w.with_chain_id(chain_id),
        Err(e) => {
            error!(error = %e, "invalid CUSTODIAN_PRIVKEY, credit payout disabled");
            return;
        }
    };

    let provider = match Provider::<Http>::try_from(&rpc_url) {
        Ok(p) => p,
        Err(e) => {
            error!(error = %e, rpc = %rpc_url, "invalid BSC_RPC_URL, credit payout disabled");
            return;
        }
    };

    let client = Arc::new(SignerMiddleware::new(provider, wallet));
    info!(custodian = %client.address(), token = %token_addr_str, chain_id, "Credit payout starting");

    let mut ticker = interval(POLL_INTERVAL);
    // Skip the immediate first tick so we don't poll before the rest
    // of the process is warmed up.
    ticker.tick().await;

    loop {
        ticker.tick().await;
        if let Err(e) = run_payout_cycle(&state, &client, token_addr).await {
            error!(error = %e, "credit payout cycle failed");
        }
    }
}

/// One full cycle: broadcast new cashouts, then confirm in-flight ones.
async fn run_payout_cycle(
    state: &AppState,
    client: &Arc<SignerMiddleware<Provider<Http>, LocalWallet>>,
    token_addr: Address,
) -> Result<()> {
    submit_pending(state, client, token_addr).await?;
    confirm_submitted(state, client).await?;
    Ok(())
}

/// Drain pending withdrawals: sign + broadcast, advance to `submitted`.
///
/// Per-row errors are isolated — one bad recipient address doesn't
/// stop the rest of the batch.
async fn submit_pending(
    state: &AppState,
    client: &Arc<SignerMiddleware<Provider<Http>, LocalWallet>>,
    token_addr: Address,
) -> Result<()> {
    let pool = state.db.pool();
    let rows: Vec<(Uuid, Uuid, f64, Option<String>)> = sqlx::query_as(
        "SELECT w.id, w.user_id, w.amount::float8, u.wallet_address
           FROM credit_withdrawals w
           JOIN users u ON u.id = w.user_id
          WHERE w.status = 'pending'
          ORDER BY w.requested_at ASC
          LIMIT $1",
    )
    .bind(MAX_SUBMITS_PER_TICK)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(());
    }
    info!(count = rows.len(), "submitting pending cashouts");

    let yeet = YeetToken::new(token_addr, client.clone());
    for (w_id, user_id, amount_yeet, wallet_addr_opt) in rows {
        let Some(wallet_addr) = wallet_addr_opt else {
            warn!(withdrawal_id = %w_id, "user has no wallet_address — failing");
            let _ = mark_failed_and_refund(pool, w_id, user_id, amount_yeet, "user has no wallet_address").await;
            continue;
        };
        let to: Address = match wallet_addr.parse() {
            Ok(a) => a,
            Err(e) => {
                warn!(withdrawal_id = %w_id, error = %e, "invalid user wallet_address");
                let _ = mark_failed_and_refund(pool, w_id, user_id, amount_yeet, "invalid wallet_address").await;
                continue;
            }
        };
        let amount_wei = yeet_to_wei(amount_yeet);

        match yeet.transfer(to, amount_wei).send().await {
            Ok(pending_tx) => {
                let tx_hash = format!("0x{:x}", pending_tx.tx_hash());
                if let Err(e) = sqlx::query(
                    "UPDATE credit_withdrawals SET status='submitted', tx_hash=$1 WHERE id=$2",
                )
                .bind(&tx_hash)
                .bind(w_id)
                .execute(pool)
                .await
                {
                    // Broadcast happened but the DB write failed — the
                    // tx will still confirm on-chain; ops can reconcile
                    // by looking at the custodian wallet's outbound txs.
                    error!(error = %e, withdrawal_id = %w_id, tx_hash, "failed to record submitted state after broadcast");
                }
            }
            Err(e) => {
                // Common causes: gas balance, RPC error, mempool busy.
                // Leave row pending; retry next tick.
                warn!(withdrawal_id = %w_id, error = %e, "transfer send failed; will retry next tick");
            }
        }
    }
    Ok(())
}

/// Walk `submitted` rows, look up receipts, advance to `confirmed` or
/// `failed` accordingly. A reverted tx atomically refunds the user's
/// credit so a failure can never strand value.
async fn confirm_submitted(
    state: &AppState,
    client: &Arc<SignerMiddleware<Provider<Http>, LocalWallet>>,
) -> Result<()> {
    let pool = state.db.pool();
    let rows: Vec<(Uuid, Uuid, f64, String)> = sqlx::query_as(
        "SELECT id, user_id, amount::float8, tx_hash
           FROM credit_withdrawals
          WHERE status = 'submitted' AND tx_hash IS NOT NULL
          ORDER BY requested_at ASC
          LIMIT $1",
    )
    .bind(MAX_CONFIRMS_PER_TICK)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(());
    }

    for (w_id, user_id, amount_yeet, tx_hash) in rows {
        let hash: H256 = match tx_hash.parse() {
            Ok(h) => h,
            Err(_) => {
                warn!(withdrawal_id = %w_id, tx_hash, "stored tx_hash unparseable");
                continue;
            }
        };
        match client.get_transaction_receipt(hash).await {
            Ok(Some(receipt)) => {
                if receipt.status == Some(U64::from(1)) {
                    if let Err(e) = sqlx::query(
                        "UPDATE credit_withdrawals SET status='confirmed', settled_at=NOW() WHERE id=$1",
                    )
                    .bind(w_id)
                    .execute(pool)
                    .await
                    {
                        warn!(error = %e, withdrawal_id = %w_id, "confirmed-state write failed");
                    }
                } else {
                    warn!(withdrawal_id = %w_id, "withdrawal tx reverted; refunding");
                    let _ = mark_failed_and_refund(pool, w_id, user_id, amount_yeet, "tx reverted").await;
                }
            }
            Ok(None) => {
                // Still pending in mempool — leave as submitted, recheck next tick.
            }
            Err(e) => {
                warn!(error = %e, withdrawal_id = %w_id, "receipt query failed");
            }
        }
    }
    Ok(())
}

/// Mark `w_id` as `failed`, refund the user's credit, in one
/// transaction.
///
/// Best-effort downstream — any error is bubbled up but the *worker*
/// caller logs and continues so one stuck refund doesn't block the
/// rest of the queue.
async fn mark_failed_and_refund(
    pool: &PgPool,
    w_id: Uuid,
    user_id: Uuid,
    amount: f64,
    reason: &str,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE credit_withdrawals SET status='failed', settled_at=NOW(), error_message=$1 WHERE id=$2",
    )
    .bind(reason)
    .bind(w_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE users SET yeet_credit_balance = yeet_credit_balance + $1 WHERE id = $2")
        .bind(amount)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Convert a `f64` YEET amount to `U256` wei.
///
/// Lossy for values beyond `f64` precision (~9 PYEET), fine for
/// everyday cashouts. Negative or non-finite inputs return zero —
/// they should have been rejected by validation upstream.
fn yeet_to_wei(yeet: f64) -> U256 {
    let multiplier = 1e18_f64;
    let wei_f = yeet * multiplier;
    if wei_f.is_finite() && wei_f >= 0.0 {
        U256::from(wei_f as u128)
    } else {
        U256::zero()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_yeet_to_wei() {
        assert_eq!(yeet_to_wei(1.0), U256::exp10(18));
    }

    #[test]
    fn quarter_yeet_to_wei() {
        assert_eq!(yeet_to_wei(0.25), U256::exp10(18) / 4u64);
    }

    #[test]
    fn negative_clamps_to_zero() {
        assert_eq!(yeet_to_wei(-5.0), U256::zero());
    }

    #[test]
    fn nan_clamps_to_zero() {
        assert_eq!(yeet_to_wei(f64::NAN), U256::zero());
    }
}

// Rust guideline compliant 2026-02-21
