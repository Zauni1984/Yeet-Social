//! YEET Credit primitives — locked debit + credit inside a transaction.
//!
//! YEET Credit is the platform's off-chain spendable balance, distinct
//! from the user's on-chain YEET (the latter sits at their
//! `wallet_address` on BSC and is moved by signed transactions in the
//! browser). Credit is what backs the cheap micro-actions that can't
//! afford a gas fee per call:
//!
//! - paper-wallet vouchers (issuer pre-funds, claimer redeems)
//! - PPV (pay-per-view) post unlocks
//! - DM-attached tips
//!
//! Credit enters the system via deposit ([credit_ops::deposit] —
//! planned), exits via cashout ([credit_ops::cashout] — planned), and
//! moves between users via [debit_credit_pair]. Tips between users
//! that should hit the chain go through [`crate::api::tips`] instead.
//!
//! Schema today: a single `users.yeet_credit_balance` column (renamed to
//! `yeet_credit_balance` in a forthcoming migration). Row-level locking
//! via `SELECT ... FOR UPDATE` is the only concurrency control —
//! sufficient because every caller is inside a Postgres transaction.

use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{AppError, AppResult};

/// Atomically move `amount` of YEET Credit from `from_id` to `to_id`
/// and insert a `tips` row for history.
///
/// Locks the sender's row with `SELECT ... FOR UPDATE` to prevent
/// concurrent over-spend, refuses transfers to the caller themselves,
/// refuses non-positive amounts, and refuses transfers that would
/// overdraw the sender.
///
/// Writes a `tips` row with `creator_amount = amount` and
/// `platform_cut = 0` — the off-chain fee skim from the old design is
/// retired; a future fee-on-transfer YEET v2 will surface as a
/// separate event the indexer can attribute. Returns the inserted tip
/// row's id so the caller can foreign-key it (e.g.
/// `ppv_unlocks.tip_id`, `messages.tip_id`).
///
/// The function takes a borrowed transaction so the caller can compose
/// it with other writes (the PPV unlock inserts a `ppv_unlocks` row,
/// the DM tip inserts a `messages` row) and commit them together.
///
/// # Errors
///
/// - [`AppError::Validation`] if `amount <= 0`, `from_id == to_id`, or
///   the sender's credit balance is below `amount`.
/// - [`AppError::Database`] on any underlying SQL failure (lock
///   acquisition, balance read, balance write, tips insert).
pub(crate) async fn debit_credit_pair(
    tx: &mut Transaction<'_, Postgres>,
    from_id: Uuid,
    to_id: Uuid,
    post_id: Option<Uuid>,
    amount: f64,
) -> AppResult<Uuid> {
    if !amount.is_finite() || amount <= 0.0 {
        return Err(AppError::Validation("Amount must be greater than 0".into()));
    }
    if from_id == to_id {
        return Err(AppError::Validation(
            "Cannot move credit to yourself".into(),
        ));
    }

    // Lock the sender row so two concurrent debits can't both pass the
    // balance check on the same starting balance.
    let balance: f64 = sqlx::query_scalar(
        "SELECT COALESCE(yeet_credit_balance, 0)::float8 FROM users WHERE id = $1 FOR UPDATE",
    )
    .bind(from_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::Database)?;

    if balance < amount {
        // User-facing message stays generic — internally we know it's
        // the off-chain Credit balance that ran short, but the product
        // surfaces a single YEET concept to users.
        return Err(AppError::Validation("Insufficient YEET".into()));
    }

    sqlx::query("UPDATE users SET yeet_credit_balance = yeet_credit_balance - $1 WHERE id = $2")
        .bind(amount)
        .bind(from_id)
        .execute(&mut **tx)
        .await
        .map_err(AppError::Database)?;

    sqlx::query("UPDATE users SET yeet_credit_balance = yeet_credit_balance + $1 WHERE id = $2")
        .bind(amount)
        .bind(to_id)
        .execute(&mut **tx)
        .await
        .map_err(AppError::Database)?;

    // History row so existing `tips` UI keeps rendering. `tx_hash` is
    // NULL because nothing settled on-chain.  `creator_amount = amount`,
    // `platform_cut = 0` — see module docs.
    let amount_str = format_amount(amount);
    let tip_id: Uuid = sqlx::query_scalar(
        "INSERT INTO tips (from_user_id, to_user_id, post_id, amount, creator_amount, platform_cut, currency, tx_hash)
         VALUES ($1, $2, $3, $4, $4, '0', 'YEET', NULL)
         RETURNING id",
    )
    .bind(from_id)
    .bind(to_id)
    .bind(post_id)
    .bind(&amount_str)
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::Database)?;

    Ok(tip_id)
}

/// Render an `f64` YEET amount as the decimal string the `tips.amount`
/// column expects. Trims trailing zeros after the decimal point but
/// preserves the integer portion as written.
fn format_amount(v: f64) -> String {
    // 8 decimal places matches the schema's NUMERIC(20,8). Trimming
    // trailing zeros keeps the row tidy for display.
    let raw = format!("{v:.8}");
    if let Some((whole, frac)) = raw.split_once('.') {
        let trimmed = frac.trim_end_matches('0');
        if trimmed.is_empty() {
            whole.to_string()
        } else {
            format!("{whole}.{trimmed}")
        }
    } else {
        raw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_integer() {
        assert_eq!(format_amount(5.0), "5");
    }

    #[test]
    fn format_fraction_trims_zeros() {
        assert_eq!(format_amount(1.5), "1.5");
        assert_eq!(format_amount(0.25), "0.25");
    }

    #[test]
    fn format_eight_decimals_max() {
        assert_eq!(format_amount(0.000_000_01), "0.00000001");
    }
}

// Rust guideline compliant 2026-02-21
