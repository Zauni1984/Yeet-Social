//! NOTE → YEET swap: deposit-address allocation + chain watcher.
//!
//! Design: docs/swap-note-to-yeet.md. The Note blockchain is a Bitcoin-Core-
//! style UTXO chain with no contracts, so the swap is a one-way deposit
//! bridge: a per-user deposit address on OUR Note full node, a watcher that
//! polls the node's wallet RPC, and — once a deposit has enough
//! confirmations — a YEET payout queued through the existing conversion
//! pipeline (kind='conversion', action='note_swap'). That way the admin
//! approval gate and the pool guard cover swap payouts exactly like point
//! conversions, and the batch minter pays them out unchanged.
//!
//! Everything is inert unless SWAP_ENABLED=true AND NOTE_RPC_URL is set.
use std::time::Duration;
use tokio::time::interval;
use tracing::{error, info, warn};
use uuid::Uuid;
use crate::{db::Database, error::AppResult, AppError, AppState};

/// Fixed swap rate: this many NOTE buy one YEET.
pub const NOTE_PER_YEET: f64 = 100.0;

#[derive(Debug, Clone)]
pub struct SwapConfig {
    pub enabled: bool,
    pub rpc_url: Option<String>,
    pub rpc_user: Option<String>,
    pub rpc_pass: Option<String>,
    /// Confirmations required before a deposit is credited (30 s blocks →
    /// 120 ≈ 1 h; conservative against reorgs on a small PoW chain).
    pub confirmations: i64,
    /// Total YEET the swap may ever pay out (500M ≈ 50B NOTE / 100).
    pub pool_cap_yeet: f64,
    /// Minimum NOTE per deposit that gets credited (0 = no minimum).
    pub min_note: f64,
}

fn env_f64(key: &str, default: f64) -> f64 {
    std::env::var(key).ok().and_then(|v| v.trim().parse().ok())
        .filter(|n: &f64| n.is_finite() && *n >= 0.0).unwrap_or(default)
}

pub fn config() -> SwapConfig {
    let enabled = matches!(std::env::var("SWAP_ENABLED").as_deref(), Ok("1") | Ok("true") | Ok("TRUE"));
    SwapConfig {
        enabled,
        rpc_url: std::env::var("NOTE_RPC_URL").ok().filter(|s| !s.is_empty()),
        rpc_user: std::env::var("NOTE_RPC_USER").ok(),
        rpc_pass: std::env::var("NOTE_RPC_PASS").ok(),
        confirmations: std::env::var("SWAP_CONFIRMATIONS").ok().and_then(|v| v.parse().ok()).unwrap_or(120),
        pool_cap_yeet: env_f64("SWAP_POOL_CAP_YEET", 500_000_000.0),
        min_note: env_f64("SWAP_MIN_NOTE", 0.0),
    }
}

/// Is the swap operational (flag on AND node reachable by config)?
pub fn is_live(cfg: &SwapConfig) -> bool { cfg.enabled && cfg.rpc_url.is_some() }

/// Minimal Bitcoin-Core-style JSON-RPC call against the Note node.
pub async fn rpc(cfg: &SwapConfig, method: &str, params: serde_json::Value) -> anyhow::Result<serde_json::Value> {
    let url = cfg.rpc_url.clone().ok_or_else(|| anyhow::anyhow!("NOTE_RPC_URL not configured"))?;
    let client = reqwest::Client::builder().timeout(Duration::from_secs(20)).build()?;
    let mut req = client.post(&url).json(&serde_json::json!({
        "jsonrpc": "1.0", "id": "yeet-swap", "method": method, "params": params
    }));
    if let Some(u) = &cfg.rpc_user { req = req.basic_auth(u, cfg.rpc_pass.as_deref()); }
    let body: serde_json::Value = req.send().await?.error_for_status()?.json().await?;
    if let Some(err) = body.get("error").filter(|e| !e.is_null()) {
        anyhow::bail!("note rpc {method}: {err}");
    }
    Ok(body.get("result").cloned().unwrap_or(serde_json::Value::Null))
}

/// Return the user's personal NOTE deposit address, allocating one from the
/// node (label = user id, so the node-side wallet stays auditable) on first
/// request. One address per user, one user per address (DB-enforced).
pub async fn allocate_address(db: &Database, cfg: &SwapConfig, user_id: Uuid) -> AppResult<String> {
    if let Some(addr) = sqlx::query_scalar::<_, String>(
        "SELECT note_address FROM swap_addresses WHERE user_id = $1"
    ).bind(user_id).fetch_optional(db.pool()).await.map_err(AppError::Database)? {
        return Ok(addr);
    }
    let v = rpc(cfg, "getnewaddress", serde_json::json!([user_id.to_string()])).await
        .map_err(|e| AppError::Internal(format!("note node: {e}")))?;
    let addr = v.as_str().filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Internal("note node returned no address".into()))?
        .to_string();
    sqlx::query("INSERT INTO swap_addresses (user_id, note_address) VALUES ($1, $2)
                 ON CONFLICT (user_id) DO NOTHING")
        .bind(user_id).bind(&addr)
        .execute(db.pool()).await.map_err(AppError::Database)?;
    // Re-read: a concurrent first request may have won the insert.
    let final_addr: String = sqlx::query_scalar("SELECT note_address FROM swap_addresses WHERE user_id = $1")
        .bind(user_id).fetch_one(db.pool()).await.map_err(AppError::Database)?;
    Ok(final_addr)
}

/// Background watcher: every 60 s, pull recent wallet receipts from the node,
/// upsert them as deposits, and credit the ones that reached the
/// confirmation threshold. No-op (with a one-time log line) while disabled.
pub async fn start_swap_watcher(state: AppState) {
    let cfg = config();
    if !is_live(&cfg) {
        info!("note-swap: watcher idle (SWAP_ENABLED={} , NOTE_RPC_URL set={})", cfg.enabled, cfg.rpc_url.is_some());
        return;
    }
    info!("note-swap: watcher active — {} confirmations, cap {} YEET", cfg.confirmations, cfg.pool_cap_yeet);
    let mut tick = interval(Duration::from_secs(60));
    loop {
        tick.tick().await;
        if let Err(e) = scan(&state, &cfg).await { error!("note-swap: scan failed: {e}"); }
        if let Err(e) = credit_confirmed(&state, &cfg).await { error!("note-swap: credit failed: {e}"); }
    }
}

/// Bitcoin-Core `listtransactions "*" count skip include_watchonly` →
/// "receive" entries carry address / txid / vout / amount / confirmations.
async fn scan(state: &AppState, cfg: &SwapConfig) -> anyhow::Result<()> {
    let txs = rpc(cfg, "listtransactions", serde_json::json!(["*", 500, 0, true])).await?;
    let Some(list) = txs.as_array() else { return Ok(()); };
    for t in list {
        if t.get("category").and_then(|c| c.as_str()) != Some("receive") { continue; }
        let (Some(addr), Some(txid)) = (t.get("address").and_then(|a| a.as_str()), t.get("txid").and_then(|a| a.as_str())) else { continue; };
        let vout = t.get("vout").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let amount = t.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let conf = t.get("confirmations").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        if amount <= 0.0 { continue; }
        let Some(user_id) = sqlx::query_scalar::<_, Uuid>(
            "SELECT user_id FROM swap_addresses WHERE note_address = $1"
        ).bind(addr).fetch_optional(state.db.pool()).await? else { continue; };
        sqlx::query(
            "INSERT INTO swap_deposits (user_id, note_address, txid, vout, amount_note, confirmations)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (txid, vout) DO UPDATE
               SET confirmations = EXCLUDED.confirmations, updated_at = NOW()"
        )
        .bind(user_id).bind(addr).bind(txid).bind(vout).bind(amount).bind(conf)
        .execute(state.db.pool()).await?;
    }
    Ok(())
}

/// Credit every 'seen' deposit past the confirmation threshold: queue a
/// YEET payout (amount_note / 100) through the conversion pipeline and
/// record the NOTE inflow in the ledger. Atomic per deposit; idempotent via
/// the status flip inside the same transaction.
async fn credit_confirmed(state: &AppState, cfg: &SwapConfig) -> anyhow::Result<()> {
    let due: Vec<(Uuid, Uuid, f64)> = sqlx::query_as(
        "SELECT id, user_id, amount_note::float8 FROM swap_deposits
          WHERE status = 'seen' AND confirmations >= $1 ORDER BY created_at ASC LIMIT 200"
    ).bind(cfg.confirmations as i32).fetch_all(state.db.pool()).await?;

    for (dep_id, user_id, amount_note) in due {
        let mut tx = state.db.pool().begin().await?;
        // Re-check under lock (the row may have been credited by a previous tick).
        let still: Option<(String,)> = sqlx::query_as(
            "SELECT status FROM swap_deposits WHERE id = $1 FOR UPDATE"
        ).bind(dep_id).fetch_optional(&mut *tx).await?;
        if still.map(|s| s.0) != Some("seen".into()) { continue; }

        let fail = |why: &str| why.to_string();
        // Guards: minimum, swap pool cap, linked payout wallet.
        let mut err: Option<String> = None;
        if cfg.min_note > 0.0 && amount_note < cfg.min_note { err = Some(fail("below_minimum")); }
        let yeet = (amount_note / NOTE_PER_YEET * 1e8).floor() / 1e8;
        if err.is_none() {
            let paid: f64 = sqlx::query_scalar::<_, f64>(
                "SELECT COALESCE(SUM(amount), 0)::float8 FROM token_rewards
                  WHERE kind = 'conversion' AND action = 'note_swap' AND status <> 'rejected'"
            ).fetch_one(&mut *tx).await?;
            if paid + yeet > cfg.pool_cap_yeet { err = Some(fail("swap_pool_cap_reached")); }
        }
        if err.is_none() {
            let wallet: Option<String> = sqlx::query_scalar("SELECT wallet_address FROM users WHERE id = $1")
                .bind(user_id).fetch_one(&mut *tx).await?;
            if wallet.is_none() { err = Some(fail("no_wallet_linked")); }
        }
        if let Some(e) = err {
            sqlx::query("UPDATE swap_deposits SET status = 'failed', last_error = $2, updated_at = NOW() WHERE id = $1")
                .bind(dep_id).bind(&e).execute(&mut *tx).await?;
            tx.commit().await?;
            warn!("note-swap: deposit {dep_id} not credited: {e}");
            continue;
        }

        // Queue the YEET payout exactly like a points conversion: awaiting
        // manual admin approval, then minted by the batch job; counts against
        // the conversion pool via pool_status (kind='conversion').
        let payout_id: Uuid = sqlx::query_scalar(
            "INSERT INTO token_rewards (user_id, action, amount, status, kind)
             VALUES ($1, 'note_swap', $2, 'awaiting_approval', 'conversion') RETURNING id"
        ).bind(user_id).bind(yeet).fetch_one(&mut *tx).await?;

        crate::services::ledger::record_in_tx(&mut tx, crate::services::ledger::NewEntry {
            tx_type: crate::services::ledger::tx_type::NOTE_SWAP_IN.into(),
            asset: crate::services::ledger::asset::NOTE.into(),
            amount: amount_note,
            user_id: Some(user_id),
            reference_type: Some("swap_deposit".into()),
            reference_id: Some(dep_id.to_string()),
            description: Some(format!("NOTE swap deposit {amount_note} NOTE → {yeet} YEET queued (payout {payout_id})")),
            ..Default::default()
        }).await.map_err(|e| anyhow::anyhow!("ledger: {e}"))?;

        sqlx::query("UPDATE swap_deposits SET status = 'credited', payout_id = $2, updated_at = NOW() WHERE id = $1")
            .bind(dep_id).bind(payout_id).execute(&mut *tx).await?;
        tx.commit().await?;
        info!("note-swap: credited deposit {dep_id}: {amount_note} NOTE → {yeet} YEET (payout {payout_id})");
    }
    Ok(())
}
