//! On-chain Transfer-event indexer for the YEET token.
//!
//! Polls a BSC RPC node every [`POLL_INTERVAL`] seconds, asks for any
//! Transfer logs emitted by the YEET token contract since the last
//! checkpoint, and turns each one into:
//!
//! - A row in the `tips` table (only when both sender and recipient are
//!   known Yeet users, since `tips.from_user_id` is `NOT NULL`).
//! - A notification on the recipient's feed.
//!
//! Frontend in-app tips also POST `/api/v1/tips` immediately after
//! broadcast for instant UI feedback; the indexer dedupes those by
//! `tx_hash` so the recipient never sees a duplicate notification.
//!
//! For off-platform tips (someone sends YEET to a Yeet user's address
//! via MetaMask directly), the indexer is the only path that surfaces
//! them — there is no in-app POST and the tip row is skipped (sender
//! has no `user_id`), but the recipient still gets a notification
//! showing a truncated 0x… address.
//!
//! The checkpoint lives in `indexer_state.last_block` so a backend
//! restart resumes exactly where it left off.

use std::sync::Arc;

use anyhow::{Result, anyhow};
use ethers::prelude::*;
use ethers::types::{Address, BlockNumber, Filter, H256, Log, U256};
use sqlx::PgPool;
use tokio::time::{Duration, interval};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::AppState;

/// `keccak256("Transfer(address,address,uint256)")` — the ERC-20 /
/// BEP-20 Transfer event topic. Indexed by every EVM token contract.
const TRANSFER_TOPIC: &str =
    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

/// Per-poll cap on the block range requested from `eth_getLogs`.
///
/// BSC public RPCs typically allow up to 5_000 blocks per call; a
/// tighter cap reduces the cost of any single failed call and means a
/// resumed-from-far-behind indexer doesn't try to swallow the entire
/// gap in one request.
const MAX_BLOCK_RANGE: u64 = 2_000;

/// Storage key for this indexer's last-processed-block checkpoint.
///
/// Bump the suffix if the schema or semantics change and old
/// checkpoints would be invalid.
const INDEXER_KEY: &str = "yeet_transfer_v1";

/// How often to ask the RPC node for new logs.
///
/// BSC produces a block every ~3 s; 15 s polling means tips show up
/// within a few blocks of confirmation, which is well below the
/// threshold where social UX feels laggy.
const POLL_INTERVAL: Duration = Duration::from_secs(15);

/// How many decimal places of YEET to display in notification text.
const DISPLAY_DECIMALS: usize = 4;

/// Number of decimal places the YEET BEP-20 token uses.
const YEET_DECIMALS: u32 = 18;

/// Fallback BSC RPC endpoint when `BSC_RPC_URL` is not set.
///
/// Tracks the value in `backend/src/blockchain.rs::BSC_MAINNET_RPC`,
/// kept inline here so the indexer doesn't depend on the (currently
/// unwired) `blockchain` module.
const DEFAULT_BSC_RPC: &str = "https://bsc-dataseed.bnbchain.org";

/// Fallback YEET token address when `YEET_TOKEN_ADDRESS` is not set.
///
/// Tracks `backend/src/blockchain.rs::YEET_TOKEN_ADDRESS`; bump both
/// in lockstep when the deployment changes.
const DEFAULT_YEET_TOKEN_ADDRESS: &str = "0x0f1963829c6cc7A925E2F46949C3b248D69297c7";

/// Spawn-and-loop entry point. Called from `main.rs` as a background
/// `tokio::spawn` task.
///
/// Returns only if RPC/token config is invalid at startup — once the
/// loop has started, transient failures are logged and the next tick
/// retries. The function never panics deliberately; any panic in the
/// polling future would kill the task and would be a bug to fix.
pub async fn start_transfer_indexer_job(state: AppState) {
    let rpc_url = std::env::var("BSC_RPC_URL").unwrap_or_else(|_| DEFAULT_BSC_RPC.into());
    let token_addr_str =
        std::env::var("YEET_TOKEN_ADDRESS").unwrap_or_else(|_| DEFAULT_YEET_TOKEN_ADDRESS.into());

    let token_addr: Address = match token_addr_str.parse() {
        Ok(a) => a,
        Err(e) => {
            error!(error = %e, addr = %token_addr_str, "invalid YEET_TOKEN_ADDRESS, indexer disabled");
            return;
        }
    };

    let provider = match Provider::<Http>::try_from(&rpc_url) {
        Ok(p) => Arc::new(p),
        Err(e) => {
            error!(error = %e, rpc = %rpc_url, "invalid BSC_RPC_URL, indexer disabled");
            return;
        }
    };

    info!(rpc = %rpc_url, token = %token_addr_str, "Transfer indexer starting");

    let mut ticker = interval(POLL_INTERVAL);
    // The first tick fires immediately; skip it so we don't double-poll
    // on startup before the database connection is warm.
    ticker.tick().await;

    loop {
        ticker.tick().await;
        if let Err(e) = poll_once(&state, &provider, token_addr).await {
            error!(error = %e, "indexer poll failed");
        }
    }
}

/// One scan cycle. Reads the checkpoint, asks for any new logs up to
/// at most [`MAX_BLOCK_RANGE`] blocks ahead, processes each, and
/// advances the checkpoint on success.
async fn poll_once(
    state: &AppState,
    provider: &Provider<Http>,
    token_addr: Address,
) -> Result<()> {
    let pool = state.db.pool();

    let current = provider.get_block_number().await?.as_u64();
    let last_seen = get_last_seen(pool)
        .await?
        // First-run: don't index the entire chain history. Start from
        // the current head and only catch new tips going forward.
        .unwrap_or_else(|| current.saturating_sub(1));

    if current <= last_seen {
        return Ok(());
    }

    let from_block = last_seen.saturating_add(1);
    let to_block = current.min(from_block.saturating_add(MAX_BLOCK_RANGE - 1));

    let transfer_topic: H256 = TRANSFER_TOPIC
        .parse()
        .map_err(|e| anyhow!("bad transfer topic literal: {e}"))?;

    let filter = Filter::new()
        .address(token_addr)
        .topic0(transfer_topic)
        .from_block(BlockNumber::Number(from_block.into()))
        .to_block(BlockNumber::Number(to_block.into()));

    let logs = provider.get_logs(&filter).await?;

    if !logs.is_empty() {
        info!(from_block, to_block, count = logs.len(), "indexed Transfer logs");
    }

    for log in &logs {
        if let Err(e) = process_log(pool, log).await {
            warn!(error = %e, "process_log failed");
        }
    }

    set_last_seen(pool, to_block).await?;
    Ok(())
}

/// Turn a single Transfer log into a deposit credit, a tip row + notification,
/// or a no-op depending on what the log carries.
///
/// The function is idempotent on `tx_hash`: a second call with the
/// same log is a no-op (deposit dedupes on `credit_deposits.tx_hash`,
/// tip dedupes on `tips.tx_hash`), so it's safe to re-process a block
/// range if the checkpoint update racy-rolls back.
async fn process_log(pool: &PgPool, log: &Log) -> Result<()> {
    // A well-formed Transfer event always has 3 topics: the event
    // signature plus indexed `from` and `to`. Anything else is either
    // a malformed log or a non-Transfer event that slipped past the
    // filter — skip silently.
    if log.topics.len() < 3 {
        return Ok(());
    }

    let from_addr = topic_to_address(&log.topics[1]);
    let to_addr = topic_to_address(&log.topics[2]);
    let amount_wei = decode_amount(&log.data);

    let tx_hash = log
        .transaction_hash
        .map(|h| format!("0x{h:064x}"))
        .ok_or_else(|| anyhow!("Transfer log without tx_hash"))?;

    let from_addr_lc = format!("0x{from_addr:040x}").to_lowercase();
    let to_addr_lc = format!("0x{to_addr:040x}").to_lowercase();

    // Deposit branch — if the recipient is the platform custodian, this
    // Transfer is a user funding their YEET Credit balance, not a
    // peer-to-peer tip. Credit it, notify, short-circuit.
    //
    // Nested `if let` (rather than a let-chain) keeps the file
    // edition-2021 compatible — backend hasn't migrated to 2024.
    if let Some(deposit_lc) = deposit_address_lc() {
        if to_addr_lc == deposit_lc {
            return process_deposit(pool, &from_addr_lc, amount_wei, &tx_hash).await;
        }
    }

    // Dedupe — the frontend POSTs `/api/v1/tips` immediately after a
    // successful broadcast, so by the time the indexer sees the log
    // the tip row may already exist.
    let already: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM tips WHERE tx_hash = $1 LIMIT 1")
            .bind(&tx_hash)
            .fetch_optional(pool)
            .await?;
    if already.is_some() {
        return Ok(());
    }

    let to_user_id: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE LOWER(wallet_address) = $1")
            .bind(&to_addr_lc)
            .fetch_optional(pool)
            .await?;

    // Recipient isn't a Yeet user — nothing to surface to anyone.
    let Some(to_user_id) = to_user_id else {
        return Ok(());
    };

    let from_user_id: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE LOWER(wallet_address) = $1")
            .bind(&from_addr_lc)
            .fetch_optional(pool)
            .await?;

    let amount_display = format_token_amount(amount_wei);

    // Only record a tip row when both parties are known — the `tips`
    // table's `from_user_id` column is NOT NULL.
    if let Some(from_id) = from_user_id {
        if let Err(e) = sqlx::query(
            "INSERT INTO tips (from_user_id, to_user_id, post_id, amount, creator_amount, platform_cut, currency, tx_hash)
             VALUES ($1, $2, NULL, $3, $3, '0', 'YEET', $4)",
        )
        .bind(from_id)
        .bind(to_user_id)
        .bind(&amount_display)
        .bind(&tx_hash)
        .execute(pool)
        .await
        {
            warn!(error = %e, tx_hash, "tip insert failed");
        }
    }

    let sender_label = sender_label(pool, from_user_id, &from_addr_lc).await;
    let message = format!("{sender_label} tipped you {amount_display} YEET");
    crate::api::notifications::notify(pool, to_user_id, from_user_id, "tip", &message, None).await;

    Ok(())
}

/// Credit a user for a confirmed YEET deposit to the custodian address.
///
/// Idempotent on `tx_hash` via the `credit_deposits` unique constraint.
/// Skips silently when the sender isn't a registered Yeet user — an
/// orphan deposit can be reconciled manually later (we have the tx
/// hash and the depositor address, so support tooling can resolve it).
///
/// Notifies the depositor so they see the credit in their feed without
/// having to refresh the wallet card.
async fn process_deposit(
    pool: &PgPool,
    from_addr_lc: &str,
    amount_wei: U256,
    tx_hash: &str,
) -> Result<()> {
    let depositor_id: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE LOWER(wallet_address) = $1")
            .bind(from_addr_lc)
            .fetch_optional(pool)
            .await?;
    let Some(depositor_id) = depositor_id else {
        // Orphan deposit — log it for ops visibility and move on.
        warn!(tx_hash, from = %from_addr_lc, "deposit from unknown wallet — orphaned");
        return Ok(());
    };

    let amount_display = format_token_amount(amount_wei);
    let amount_f64: f64 = amount_display.parse().unwrap_or(0.0);

    let inserted: Option<(Uuid,)> = sqlx::query_as(
        "INSERT INTO credit_deposits (user_id, tx_hash, amount)
         VALUES ($1, $2, $3)
         ON CONFLICT (tx_hash) DO NOTHING
         RETURNING id",
    )
    .bind(depositor_id)
    .bind(tx_hash)
    .bind(amount_f64)
    .fetch_optional(pool)
    .await?;

    if inserted.is_none() {
        // Already credited — frontend POST got there first, or this
        // log was reprocessed. Either way, nothing to do.
        return Ok(());
    }

    if let Err(e) = sqlx::query(
        "UPDATE users SET yeet_credit_balance = yeet_credit_balance + $1 WHERE id = $2",
    )
    .bind(amount_f64)
    .bind(depositor_id)
    .execute(pool)
    .await
    {
        warn!(error = %e, tx_hash, "deposit balance update failed");
    }

    let message = format!("Deposit confirmed: {amount_display} YEET credited");
    crate::api::notifications::notify(pool, depositor_id, None, "credit_deposit", &message, None)
        .await;

    Ok(())
}

/// Read the configured custodian deposit address, normalised to lowercase
/// `0x…` for direct string comparison with log topics. Returns `None`
/// when `YEET_DEPOSIT_ADDRESS` is unset or malformed — the deposit
/// branch silently skips in that case, which is the right thing
/// pre-launch (no custodian configured = no deposits to detect).
fn deposit_address_lc() -> Option<String> {
    let raw = std::env::var("YEET_DEPOSIT_ADDRESS").ok()?;
    let trimmed = raw.trim();
    let body = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    if body.len() != 40 || !body.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("0x{}", body.to_ascii_lowercase()))
}

/// Resolve a display label for the sender of a tip.
///
/// Falls back to a short `0xabcd…1234` address when the sender isn't
/// a registered Yeet user (off-platform tip arriving from MetaMask
/// directly).
async fn sender_label(pool: &PgPool, from_user_id: Option<Uuid>, from_addr_lc: &str) -> String {
    if let Some(uid) = from_user_id {
        let label: Option<String> = sqlx::query_scalar(
            "SELECT COALESCE(display_name, username) FROM users WHERE id = $1",
        )
        .bind(uid)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
        if let Some(l) = label {
            return l;
        }
    }
    let len = from_addr_lc.len();
    if len >= 10 {
        format!("{}…{}", &from_addr_lc[..6], &from_addr_lc[len - 4..])
    } else {
        from_addr_lc.to_string()
    }
}

/// Convert a 32-byte topic (left-padded address) into a 20-byte `Address`.
fn topic_to_address(topic: &H256) -> Address {
    let bytes = topic.as_bytes();
    Address::from_slice(&bytes[12..32])
}

/// Decode the 32-byte `uint256` amount field from a Transfer log's data.
fn decode_amount(data: &[u8]) -> U256 {
    if data.len() >= 32 {
        U256::from_big_endian(&data[..32])
    } else {
        U256::zero()
    }
}

/// Format a wei amount as a human-readable YEET string with up to
/// [`DISPLAY_DECIMALS`] decimal places (trailing zeros trimmed).
fn format_token_amount(wei: U256) -> String {
    let denom = U256::exp10(YEET_DECIMALS as usize);
    let whole = wei / denom;
    let frac = wei % denom;
    if frac.is_zero() {
        return whole.to_string();
    }
    let frac_padded = format!("{frac:0>18}");
    let head: String = frac_padded.chars().take(DISPLAY_DECIMALS).collect();
    let trimmed = head.trim_end_matches('0');
    if trimmed.is_empty() {
        whole.to_string()
    } else {
        format!("{whole}.{trimmed}")
    }
}

/// Read the last-processed block from the checkpoint table.
async fn get_last_seen(pool: &PgPool) -> Result<Option<u64>> {
    let last: Option<i64> =
        sqlx::query_scalar("SELECT last_block FROM indexer_state WHERE indexer_key = $1")
            .bind(INDEXER_KEY)
            .fetch_optional(pool)
            .await?;
    Ok(last.map(|n| n as u64))
}

/// Advance the checkpoint to `block` (upsert).
async fn set_last_seen(pool: &PgPool, block: u64) -> Result<()> {
    sqlx::query(
        "INSERT INTO indexer_state (indexer_key, last_block, updated_at)
         VALUES ($1, $2, NOW())
         ON CONFLICT (indexer_key) DO UPDATE SET last_block = EXCLUDED.last_block, updated_at = NOW()",
    )
    .bind(INDEXER_KEY)
    .bind(block as i64)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_amount_whole_yeet() {
        let one = U256::exp10(18);
        assert_eq!(format_token_amount(one), "1");
    }

    #[test]
    fn format_amount_quarter() {
        let quarter = U256::exp10(18) / 4u64;
        assert_eq!(format_token_amount(quarter), "0.25");
    }

    #[test]
    fn format_amount_truncates_to_four_decimals() {
        // 1.234567 YEET → "1.2345" (4-decimal cap, trailing-zero trimmed).
        let amount = U256::exp10(18) + (U256::exp10(18) * 234_567u64) / 1_000_000u64;
        assert_eq!(format_token_amount(amount), "1.2345");
    }

    #[test]
    fn topic_address_extraction() {
        // Address `0xaabbccddeeff00112233445566778899aabbccdd` left-padded
        // to 32 bytes lands in `topics[1]` / `topics[2]` of a Transfer log.
        let mut bytes = [0u8; 32];
        bytes[12..].copy_from_slice(&[
            0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
            0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
        ]);
        let topic = H256::from_slice(&bytes);
        let addr = topic_to_address(&topic);
        assert_eq!(
            format!("0x{addr:040x}"),
            "0xaabbccddeeff00112233445566778899aabbccdd"
        );
    }
}

// Rust guideline compliant 2026-02-21
