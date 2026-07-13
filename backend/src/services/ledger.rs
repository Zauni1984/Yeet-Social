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
    pub const TIP_SENT: &str            = "tip_sent";            // points tip debited from sender
    pub const TIP_RECEIVED: &str        = "tip_received";        // points tip credited to creator
    pub const PPV_PURCHASE: &str        = "ppv_purchase";        // points spent to unlock PPV
    pub const PPV_EARNING: &str         = "ppv_earning";         // points earned by PPV author
    pub const PLATFORM_FEE: &str        = "platform_fee";        // platform cut (points)
    pub const PAPER_WALLET_ISSUE: &str  = "paper_wallet_issue";  // points locked into a voucher
    pub const PAPER_WALLET_CLAIM: &str  = "paper_wallet_claim";  // points released to redeemer
    pub const PAPER_WALLET_REFUND: &str = "paper_wallet_refund"; // points returned to issuer
    pub const POINTS_CONVERSION: &str   = "points_conversion";   // points debited for a YEET payout
    pub const ONCHAIN_PAYOUT: &str      = "onchain_payout";      // YEET minted to a user wallet
    pub const ONCHAIN_TIP: &str         = "onchain_tip";         // on-chain YEET tip (indexer)
    pub const ONCHAIN_PPV: &str         = "onchain_ppv";         // on-chain YEET PPV (indexer)
}

pub mod asset {
    pub const POINTS: &str = "POINTS";
    pub const YEET: &str   = "YEET";
    pub const BNB: &str    = "BNB";
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
