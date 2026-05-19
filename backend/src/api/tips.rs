//! Tip recording.
//!
//! Yeet's tip flow is fully non-custodial: the actual YEET transfer is
//! an on-chain BEP-20 transaction signed in the user's browser (either
//! by the in-app DontYeetWallet or by MetaMask). This module no longer
//! moves any balance — there is no off-chain ledger. Its job is:
//!
//! 1. Accept the broadcast `tx_hash` from the frontend so the recipient
//!    sees a notification immediately, before the
//!    [`crate::services::indexer`] chain-watcher gets to it.
//! 2. Insert a tip row for the sender's "my tips" history view.
//!
//! The chain is the source of truth for whether the tip actually
//! settled. The indexer reconciles on-chain Transfer events with the
//! rows written here; tips that the frontend reports but the chain
//! never settles will sit as orphan rows (we can sweep them later).

use axum::{Json, extract::State};
use serde::Deserialize;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::api::middleware::AuthUser;
use crate::{AppError, AppResult, AppState, models::ApiResponse};

#[derive(Debug, Deserialize)]
pub struct SendTipRequest {
    /// Recipient — either a Yeet `user_id` (UUID) or an EVM wallet address.
    pub to_address: String,
    /// Decimal YEET amount as displayed (e.g. `"5"` or `"2.5"`).
    pub amount: String,
    /// Optional post the tip is tied to (so the post composer can show "tipped").
    pub post_id: Option<Uuid>,
    /// The on-chain transaction hash returned by `eth_sendRawTransaction`.
    ///
    /// Required for the row to be created — without it we'd be writing
    /// an unverifiable claim. A frontend that fails to broadcast should
    /// not POST here.
    pub tx_hash: Option<String>,
}

/// Insert a tip row inside the caller's transaction.
///
/// Used both by the HTTP route (which requires `tx_hash` upstream — see
/// [`send_tip`]) and by intra-app callers like `api::messages` (DM tip
/// attachments) and `api::posts` (PPV unlocks) that don't yet pass an
/// on-chain tx hash. The DB row is purely for history/UX; the chain is
/// authoritative, so rows without a `tx_hash` are tolerated for legacy
/// internal flows but no longer accepted from the public endpoint.
///
/// # Errors
///
/// - [`AppError::Validation`] if `amount` doesn't parse to a positive
///   decimal or the sender is tipping themselves.
/// - [`AppError::Database`] on insert failure.
pub(crate) async fn send_tip_tx(
    tx: &mut Transaction<'_, Postgres>,
    from_id: Uuid,
    to_id: Uuid,
    post_id: Option<Uuid>,
    amount_str: &str,
    tx_hash: Option<&str>,
) -> AppResult<Uuid> {
    let amount_val: f64 = amount_str.parse().unwrap_or(0.0);
    if amount_val <= 0.0 {
        return Err(AppError::Validation("Amount must be greater than 0".into()));
    }
    if from_id == to_id {
        return Err(AppError::Validation("Cannot tip yourself".into()));
    }
    let tx_hash = tx_hash.filter(|h| !h.is_empty());

    // Insert the tip row. `creator_amount` mirrors `amount` and
    // `platform_cut` is zero — the old 10% off-chain fee skim is gone
    // along with the rest of the off-chain ledger. A future on-chain
    // fee mechanism (a fee-on-transfer YEET v2 or a relayer skim)
    // would surface as a separate event the indexer can attribute.
    let tip_id: Uuid = sqlx::query_scalar(
        "INSERT INTO tips (from_user_id, to_user_id, post_id, amount, creator_amount, platform_cut, currency, tx_hash)
         VALUES ($1, $2, $3, $4, $4, '0', 'YEET', $5)
         RETURNING id",
    )
    .bind(from_id)
    .bind(to_id)
    .bind(post_id)
    .bind(amount_str)
    .bind(tx_hash)
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::Database)?;

    Ok(tip_id)
}

pub async fn send_tip(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<SendTipRequest>,
) -> AppResult<Json<ApiResponse<Uuid>>> {
    // Resolve sender id
    let from_id: Uuid = if auth.address.starts_with("email:") {
        let id_str = auth.address.trim_start_matches("email:");
        Uuid::parse_str(id_str).map_err(|_| AppError::Validation("Invalid user id".into()))?
    } else {
        sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
            .bind(&auth.address)
            .fetch_optional(state.db.pool())
            .await
            .map_err(AppError::Database)?
            .ok_or_else(|| AppError::NotFound("Sender not found".into()))?
    };

    // Resolve recipient (UUID or wallet)
    let to_id: Uuid = if let Ok(uid) = Uuid::parse_str(&req.to_address) {
        uid
    } else {
        sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
            .bind(req.to_address.to_lowercase())
            .fetch_optional(state.db.pool())
            .await
            .map_err(AppError::Database)?
            .ok_or_else(|| AppError::NotFound("Recipient not found".into()))?
    };

    // Refuse if either party has blocked the other.
    if crate::api::blocks::either_blocks(state.db.pool(), from_id, to_id).await? {
        return Err(AppError::Forbidden("Blocked".into()));
    }

    // Public endpoint requires an on-chain tx_hash — without it we'd
    // be writing an unverifiable claim. Internal callers (DM tips, PPV
    // unlocks) can still post None via `send_tip_tx` directly.
    let tx_hash = req
        .tx_hash
        .as_deref()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| AppError::Validation("Missing on-chain tx_hash".into()))?;

    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;
    let tip_id = send_tip_tx(
        &mut tx,
        from_id,
        to_id,
        req.post_id,
        &req.amount,
        Some(tx_hash),
    )
    .await?;
    tx.commit().await.map_err(AppError::Database)?;

    // Optimistic notification: the chain-watcher would also surface
    // this, but firing it here means the recipient sees the toast
    // within a frame of the sender's confirmation rather than after
    // the next indexer poll. The indexer dedupes by `tx_hash`.
    let actor = sqlx::query_scalar::<_, Option<String>>(
        "SELECT COALESCE(display_name, username) FROM users WHERE id = $1",
    )
    .bind(from_id)
    .fetch_optional(state.db.pool())
    .await
    .ok()
    .flatten()
    .flatten()
    .unwrap_or_else(|| "Someone".into());
    crate::api::notifications::notify(
        state.db.pool(),
        to_id,
        Some(from_id),
        "tip",
        &format!("{actor} tipped you {} YEET", req.amount),
        req.post_id,
    )
    .await;

    Ok(Json(ApiResponse::ok(tip_id)))
}

// Rust guideline compliant 2026-02-21
