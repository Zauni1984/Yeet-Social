//! Append-only, tamper-evident transaction ledger.
//!
//! Every value movement in the system (points + on-chain) is appended here as
//! an immutable, hash-chained entry. The ledger is the single source of truth
//! for audits, evidence (Nachweis) and tax (Finanzamt) exports.
//!
//! Guarantees:
//! - append-only: DB triggers block UPDATE/DELETE (migration 0039)
//! - gapless entry_no: assigned under a transaction advisory lock
//! - hash chain: entry_hash = sha256(canonical || prev_hash), so tampering
//!   with any historical row breaks every subsequent hash
//!
//! Recording joins the caller's DB transaction (`record_in_tx`) so the ledger
//! entry and the balance change commit atomically — a movement can never be
//! applied without being recorded, and vice versa.
#![allow(dead_code)]
use sqlx::{Postgres, Transaction, PgPool};
use uuid::Uuid;
use sha2::{Digest, Sha256};
use crate::{AppError, AppResult};

/// Advisory-lock key that serialises ledger appends (keeps entry_no gapless
/// and the hash chain linear). Released automatically at tx commit/rollback.
const LEDGER_LOCK_KEY: i64 = 0x59_45_45_54_4c_44_47; // "YEETLDG"

/// Canonical transaction types. Add new kinds here so exports stay complete.
pub mod tx_type {
    pub const REWARD_GRANT: &str        = "reward_grant";        // engagement points earned
    pub const REGISTRATION_BONUS: &str  = "registration_bonus";  // one-time signup bonus (points)
    pub const TIP_SENT: &str            = "tip_sent";            // points tip debited from sender
    pub const TIP_RECEIVED: &str        = "tip_received";        // points tip credited to creator
    pub const PPV_PURCHASE: &str        = "ppv_purchase";        // points spent to unlock PPV
    pub const PPV_EARNING: &str         = "ppv_earning";         // points earned by PPV author
    pub const PLATFORM_FEE: &str        = "platform_fee";        // platform cut (points)
    pub const PAPER_WALLET_ISSUE: &str  = "paper_wallet_issue";  // points locked into a voucher
    pub const PAPER_WALLET_CLAIM: &str  = "paper_wallet_claim";  // points released to redeemer
    pub const PAPER_WALLET_REFUND: &str = "paper_wallet_refund"; // points returned to issuer
    pub const POINTS_CONVERSION: &str   = "points_conversion";   // points debited for a YEET payout
    pub const PAYOUT_REFUND: &str       = "payout_refund";       // points returned after an admin-rejected payout
    pub const ONCHAIN_PAYOUT: &str      = "onchain_payout";      // YEET minted to a user wallet
    pub const NOTE_SWAP_IN: &str        = "note_swap_in";        // NOTE received on a swap deposit address
    pub const ONCHAIN_TIP: &str         = "onchain_tip";         // on-chain YEET tip (indexer)
    pub const ONCHAIN_PPV: &str         = "onchain_ppv";         // on-chain YEET PPV (indexer)
    pub const LIVE_PROMOTION: &str      = "live_promotion";      // points spent on a live promotion (100 % platform)
    pub const LIVE_PROMOTION_REFUND: &str = "live_promotion_refund"; // points returned for a never-applied promotion
    pub const OPENING_BALANCE: &str     = "opening_balance";     // one-time baseline for balances older than the journal
}

pub mod asset {
    pub const POINTS: &str = "POINTS";
    pub const YEET: &str   = "YEET";
    pub const BNB: &str    = "BNB";
    pub const NOTE: &str   = "NOTE";
    pub const EUR: &str    = "EUR";
}

/// A ledger append request. `amount` is signed from the subject's perspective:
/// positive = credited to `user_id`, negative = debited from `user_id`.
#[derive(Debug, Clone, Default)]
pub struct NewEntry {
    pub occurred_at: Option<chrono::DateTime<chrono::Utc>>, // defaults to now
    pub tx_type: String,
    pub asset: String,
    pub amount: f64,
    pub fee_amount: f64,
    pub user_id: Option<Uuid>,
    pub counterparty_id: Option<Uuid>,
    pub user_wallet: Option<String>,
    pub counterparty_wallet: Option<String>,
    pub reference_type: Option<String>,
    pub reference_id: Option<String>,
    pub onchain_tx_hash: Option<String>,
    pub fiat_value: Option<f64>,
    pub fx_rate: Option<f64>,
    pub fx_source: Option<String>,
    pub description: Option<String>,
    pub created_by: Option<String>,
}

fn canonical(entry_no: i64, e: &NewEntry, occurred: &chrono::DateTime<chrono::Utc>, prev_hash: &str) -> String {
    // Stable, order-fixed serialization. Any change to any field changes the
    // hash; including prev_hash chains the rows.
    format!(
        "{entry_no}|{occurred}|{tt}|{asset}|{amount:.18}|{fee:.18}|{uid}|{cid}|{uw}|{cw}|{rt}|{rid}|{tx}|{fv}|{fx}|{fxs}|{desc}|{prev}",
        entry_no = entry_no,
        occurred = occurred.timestamp_micros(),
        tt = e.tx_type,
        asset = e.asset,
        amount = e.amount,
        fee = e.fee_amount,
        uid = e.user_id.map(|u| u.to_string()).unwrap_or_default(),
        cid = e.counterparty_id.map(|u| u.to_string()).unwrap_or_default(),
        uw = e.user_wallet.clone().unwrap_or_default(),
        cw = e.counterparty_wallet.clone().unwrap_or_default(),
        rt = e.reference_type.clone().unwrap_or_default(),
        rid = e.reference_id.clone().unwrap_or_default(),
        tx = e.onchain_tx_hash.clone().unwrap_or_default(),
        fv = e.fiat_value.map(|v| format!("{v:.18}")).unwrap_or_default(),
        fx = e.fx_rate.map(|v| format!("{v:.18}")).unwrap_or_default(),
        fxs = e.fx_source.clone().unwrap_or_default(),
        desc = e.description.clone().unwrap_or_default(),
        prev = prev_hash,
    )
}

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    hex::encode(h.finalize())
}

/// Append an entry inside the caller's transaction (atomic with the balance
/// change). Returns the assigned entry_no.
pub async fn record_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    entry: NewEntry,
) -> AppResult<i64> {
    // Serialize ledger appends for this transaction so entry_no is gapless and
    // the hash chain is linear even under concurrency.
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(LEDGER_LOCK_KEY)
        .execute(&mut **tx).await.map_err(AppError::Database)?;

    let last: Option<(i64, String)> = sqlx::query_as(
        "SELECT entry_no, entry_hash FROM ledger_entries ORDER BY entry_no DESC LIMIT 1"
    )
    .fetch_optional(&mut **tx).await.map_err(AppError::Database)?;

    let (entry_no, prev_hash) = match last {
        Some((n, h)) => (n + 1, h),
        None => (1, "GENESIS".to_string()),
    };
    let occurred = entry.occurred_at.unwrap_or_else(chrono::Utc::now);
    let entry_hash = sha256_hex(&canonical(entry_no, &entry, &occurred, &prev_hash));
    let created_by = entry.created_by.clone().unwrap_or_else(|| "system".into());

    sqlx::query(
        "INSERT INTO ledger_entries
           (entry_no, occurred_at, tx_type, asset, amount, fee_amount,
            user_id, counterparty_id, user_wallet, counterparty_wallet,
            reference_type, reference_id, onchain_tx_hash,
            fiat_value, fx_rate, fx_source, description, created_by,
            prev_hash, entry_hash)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20)"
    )
    .bind(entry_no).bind(occurred).bind(&entry.tx_type).bind(&entry.asset)
    .bind(entry.amount).bind(entry.fee_amount)
    .bind(entry.user_id).bind(entry.counterparty_id)
    .bind(&entry.user_wallet).bind(&entry.counterparty_wallet)
    .bind(&entry.reference_type).bind(&entry.reference_id).bind(&entry.onchain_tx_hash)
    .bind(entry.fiat_value).bind(entry.fx_rate).bind(&entry.fx_source)
    .bind(&entry.description).bind(&created_by)
    .bind(&prev_hash).bind(&entry_hash)
    .execute(&mut **tx).await.map_err(AppError::Database)?;

    Ok(entry_no)
}

/// Append an entry in its own transaction (best-effort convenience for call
/// sites that aren't already inside a tx).
pub async fn record(pool: &PgPool, entry: NewEntry) -> AppResult<i64> {
    let mut tx = pool.begin().await.map_err(AppError::Database)?;
    let n = record_in_tx(&mut tx, entry).await?;
    tx.commit().await.map_err(AppError::Database)?;
    Ok(n)
}

#[derive(sqlx::FromRow)]
struct ChainRow {
    entry_no: i64,
    occurred_at: chrono::DateTime<chrono::Utc>,
    tx_type: String,
    asset: String,
    amount: f64,
    fee_amount: f64,
    user_id: Option<Uuid>,
    counterparty_id: Option<Uuid>,
    user_wallet: Option<String>,
    counterparty_wallet: Option<String>,
    reference_type: Option<String>,
    reference_id: Option<String>,
    onchain_tx_hash: Option<String>,
    fiat_value: Option<f64>,
    fx_rate: Option<f64>,
    fx_source: Option<String>,
    description: Option<String>,
    prev_hash: String,
    entry_hash: String,
}

/// Verify the whole hash chain (used by the admin integrity check). Returns
/// the entry_no where the chain first breaks, or None if intact.
pub async fn verify_chain(pool: &PgPool) -> AppResult<Option<i64>> {
    let rows: Vec<ChainRow> = sqlx::query_as::<_, ChainRow>(
        "SELECT entry_no, occurred_at, tx_type, asset, amount::float8, fee_amount::float8,
                user_id, counterparty_id, user_wallet, counterparty_wallet,
                reference_type, reference_id, onchain_tx_hash,
                fiat_value::float8, fx_rate::float8, fx_source, description,
                prev_hash, entry_hash
           FROM ledger_entries ORDER BY entry_no ASC"
    )
    .fetch_all(pool).await.map_err(AppError::Database)?;

    let mut expected_prev = "GENESIS".to_string();
    for r in rows {
        let e = NewEntry {
            occurred_at: Some(r.occurred_at), tx_type: r.tx_type.clone(), asset: r.asset.clone(),
            amount: r.amount, fee_amount: r.fee_amount, user_id: r.user_id, counterparty_id: r.counterparty_id,
            user_wallet: r.user_wallet.clone(), counterparty_wallet: r.counterparty_wallet.clone(),
            reference_type: r.reference_type.clone(), reference_id: r.reference_id.clone(),
            onchain_tx_hash: r.onchain_tx_hash.clone(),
            fiat_value: r.fiat_value, fx_rate: r.fx_rate, fx_source: r.fx_source.clone(),
            description: r.description.clone(), created_by: None,
        };
        let want = sha256_hex(&canonical(r.entry_no, &e, &r.occurred_at, &expected_prev));
        if r.prev_hash != expected_prev || r.entry_hash != want {
            return Ok(Some(r.entry_no));
        }
        expected_prev = r.entry_hash;
    }
    Ok(None)
}

// ───────────────────────── reconciliation (docs/mica/09 §8.1) ─────────────────────────

/// A user whose spendable balance does not equal the sum of their POINTS
/// journal entries.
#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct BalanceMismatch {
    pub user_id: Uuid,
    pub username: Option<String>,
    pub balance: f64,
    pub ledger_sum: f64,
    pub diff: f64,
}

/// Conversions that are debited but neither paid out nor refunded, per status.
#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct OpenConversions {
    pub status: String,
    pub count: i64,
    pub points: f64,
}

/// Result of the three reconciliation equations plus the hash-chain check.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Reconciliation {
    pub checked_at: chrono::DateTime<chrono::Utc>,
    /// true when no balance mismatch, no minted conversion without journal
    /// entry and an intact hash chain.
    pub ok: bool,
    /// (1) Σ POINTS journal per user == users.yeet_token_balance. Capped at
    /// `limit` rows, largest differences first; `balance_mismatch_count` is
    /// the full count.
    pub balance_mismatches: Vec<BalanceMismatch>,
    pub balance_mismatch_count: i64,
    /// (2) conversions marked `minted` that have no `onchain_payout` entry.
    pub minted_without_journal: Vec<Uuid>,
    /// (3) points debited for conversions that are still open.
    pub open_conversions: Vec<OpenConversions>,
    pub journal_entries: i64,
    /// First entry_no where the hash chain breaks (None = intact).
    pub chain_broken_at: Option<i64>,
}

const MISMATCH_SQL: &str = "
    SELECT u.id AS user_id, u.username,
           COALESCE(u.yeet_token_balance, 0)::float8 AS balance,
           COALESCE(l.s, 0)::float8 AS ledger_sum,
           (COALESCE(u.yeet_token_balance, 0) - COALESCE(l.s, 0))::float8 AS diff
      FROM users u
      LEFT JOIN (SELECT user_id, SUM(amount) AS s
                   FROM ledger_entries
                  WHERE asset = 'POINTS' AND user_id IS NOT NULL
                  GROUP BY user_id) l ON l.user_id = u.id
     WHERE ABS(COALESCE(u.yeet_token_balance, 0) - COALESCE(l.s, 0)) > 0.000001";

/// Run the reconciliation. `limit` caps the mismatch list in the response.
pub async fn reconcile(pool: &PgPool, limit: i64) -> AppResult<Reconciliation> {
    let balance_mismatches: Vec<BalanceMismatch> = sqlx::query_as(
        &format!("{MISMATCH_SQL} ORDER BY ABS(COALESCE(u.yeet_token_balance, 0) - COALESCE(l.s, 0)) DESC LIMIT $1")
    ).bind(limit.clamp(1, 10_000)).fetch_all(pool).await.map_err(AppError::Database)?;
    let balance_mismatch_count: i64 = sqlx::query_scalar(
        &format!("SELECT COUNT(*)::bigint FROM ({MISMATCH_SQL}) m")
    ).fetch_one(pool).await.map_err(AppError::Database)?;

    let minted_without_journal: Vec<Uuid> = sqlx::query_scalar(
        "SELECT r.id FROM token_rewards r
          WHERE r.kind = 'conversion' AND r.status = 'minted'
            AND NOT EXISTS (SELECT 1 FROM ledger_entries l
                             WHERE l.tx_type = 'onchain_payout' AND l.reference_id = r.id::text)
          ORDER BY r.created_at LIMIT 1000"
    ).fetch_all(pool).await.map_err(AppError::Database)?;

    let open_conversions: Vec<OpenConversions> = sqlx::query_as(
        "SELECT status, COUNT(*)::bigint AS count, COALESCE(SUM(amount), 0)::float8 AS points
           FROM token_rewards
          WHERE kind = 'conversion' AND status IN ('awaiting_approval', 'pending', 'failed')
          GROUP BY status ORDER BY status"
    ).fetch_all(pool).await.map_err(AppError::Database)?;

    let journal_entries: i64 = sqlx::query_scalar("SELECT COUNT(*)::bigint FROM ledger_entries")
        .fetch_one(pool).await.map_err(AppError::Database)?;
    let chain_broken_at = verify_chain(pool).await?;

    let ok = balance_mismatch_count == 0 && minted_without_journal.is_empty() && chain_broken_at.is_none();
    Ok(Reconciliation {
        checked_at: chrono::Utc::now(), ok,
        balance_mismatches, balance_mismatch_count,
        minted_without_journal, open_conversions, journal_entries, chain_broken_at,
    })
}

/// One-time opening balances: for every user whose balance differs from
/// their journal sum, append an `opening_balance` entry for the difference
/// so equation (1) holds from now on. Meant for balances that predate the
/// journal (migration 0039); review `reconcile()` first — a genuine bug
/// would be papered over by this. Returns the number of entries written.
pub async fn write_opening_balances(pool: &PgPool, created_by: &str) -> AppResult<i64> {
    let mut tx = pool.begin().await.map_err(AppError::Database)?;
    let rows: Vec<BalanceMismatch> = sqlx::query_as(MISMATCH_SQL)
        .fetch_all(&mut *tx).await.map_err(AppError::Database)?;
    let mut n = 0i64;
    for m in rows {
        record_in_tx(&mut tx, NewEntry {
            tx_type: tx_type::OPENING_BALANCE.into(), asset: asset::POINTS.into(),
            amount: m.diff, fee_amount: 0.0,
            user_id: Some(m.user_id),
            reference_type: Some("reconciliation".into()),
            description: Some(format!(
                "opening balance: points held before the journal existed (balance {:.6}, journal {:.6})",
                m.balance, m.ledger_sum)),
            created_by: Some(created_by.to_string()),
            ..Default::default()
        }).await?;
        n += 1;
    }
    tx.commit().await.map_err(AppError::Database)?;
    Ok(n)
}

/// Daily reconciliation job: logs a warning with the counts whenever the
/// journal and the balances disagree (docs/mica/09 D6). First run two
/// minutes after boot so a bad deploy is noticed the same day.
pub async fn start_reconcile_job(state: crate::AppState) {
    tokio::time::sleep(std::time::Duration::from_secs(120)).await;
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(24 * 3600));
    loop {
        ticker.tick().await;
        match reconcile(state.db.pool(), 20).await {
            Ok(r) if r.ok => tracing::info!(
                "ledger reconcile: ok ({} entries, chain intact, open conversions: {:?})",
                r.journal_entries,
                r.open_conversions.iter().map(|o| format!("{}={}", o.status, o.count)).collect::<Vec<_>>()
            ),
            Ok(r) => tracing::warn!(
                "ledger reconcile: MISMATCH — {} users off-balance (largest: {:?}), {} minted conversions without journal entry, chain broken at {:?}",
                r.balance_mismatch_count,
                r.balance_mismatches.first().map(|m| (m.user_id, m.diff)),
                r.minted_without_journal.len(),
                r.chain_broken_at
            ),
            Err(e) => tracing::error!("ledger reconcile failed: {e}"),
        }
    }
}
