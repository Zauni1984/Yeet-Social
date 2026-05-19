//! YEET Credit deposit + cashout HTTP handlers.
//!
//! Two endpoints move value across the on-chain ↔ off-chain boundary:
//!
//! - `POST /api/v1/credit/deposit { tx_hash }` — the user just signed
//!   and broadcast a BEP-20 `transfer` to the platform's custodian
//!   address. We pull the receipt, walk the logs, find Transfer
//!   events from the caller's `wallet_address` to the deposit address,
//!   sum the amounts, dedupe by `tx_hash`, and credit the user.
//!
//! - `POST /api/v1/credit/cashout { amount }` — the user wants their
//!   off-chain credit moved back on-chain. We atomically lock their
//!   row, debit the credit, and queue a pending withdrawal. The
//!   `services::credit_payout` worker drains the queue hourly.
//!
//! Plus two read-only history endpoints (`GET /credit/deposits`,
//! `GET /credit/withdrawals`) used by the wallet card.

use axum::{Json, extract::State};
use chrono::{DateTime, Utc};
use ethers::prelude::*;
use ethers::types::{Address, H256, U64, U256};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::middleware::AuthUser;
use crate::{AppError, AppResult, AppState, models::ApiResponse};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// `keccak256("Transfer(address,address,uint256)")` — ERC-20 / BEP-20
/// Transfer event topic. Same constant as `services::indexer`; kept
/// duplicated for now so the two modules don't grow a fragile shared
/// dependency. Promote to a `chain` module if a third caller appears.
const TRANSFER_TOPIC: &str =
    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

/// Fallback BSC RPC endpoint when `BSC_RPC_URL` is not set.
const DEFAULT_BSC_RPC: &str = "https://bsc-dataseed.bnbchain.org";

/// Fallback YEET token address when `YEET_TOKEN_ADDRESS` is not set.
const DEFAULT_YEET_TOKEN_ADDRESS: &str = "0x0f1963829c6cc7A925E2F46949C3b248D69297c7";

/// Number of decimal places the YEET BEP-20 token uses.
const YEET_DECIMALS: u32 = 18;

/// Floor on the per-request cashout size, in YEET.
///
/// Without a floor a spammer could enqueue millions of dust rows that
/// the payout worker has to walk past. The minimum is set just above
/// the cost of one BEP-20 transfer in YEET-equivalent BNB so cashout
/// is never a net-loss for the platform.
const MIN_CASHOUT_YEET: f64 = 1.0;

/// Hard ceiling per cashout request. Anything bigger goes through
/// support (anti-fat-finger guard, easy to lift later).
const MAX_CASHOUT_YEET: f64 = 1_000_000.0;

/// How many recent rows the history list endpoints return.
const HISTORY_LIMIT: i64 = 100;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

/// Static public config the frontend needs to drive the wallet card.
///
/// Returned by `GET /api/v1/credit/info`. No auth required — every
/// field is either hard-coded operator config or a constant.
#[derive(Debug, Serialize)]
pub struct CreditInfoResponse {
    /// Custodian address users send YEET to in order to fund their
    /// credit. `None` when `YEET_DEPOSIT_ADDRESS` is unset (e.g. dev
    /// environment); UI should disable the Deposit button.
    pub deposit_address: Option<String>,
    pub yeet_token_address: String,
    pub chain_id: u64,
    /// RPC endpoint the frontend should use for `eth_call`,
    /// `eth_sendRawTransaction`, etc. — matches what the backend
    /// services see, so a deposit signed and broadcast browser-side
    /// lands on the same chain the indexer is watching.
    pub bsc_rpc_url: String,
    /// Human-readable chain name for MetaMask's `wallet_addEthereumChain`
    /// prompt. Derived from `chain_id` (97 → BSC Testnet, 56 → BSC
    /// Mainnet, anything else → the chain id stringified).
    pub chain_name: String,
    /// Block-explorer base URL for tx-hash links in the UI.
    pub explorer_url: String,
    pub min_cashout: f64,
    pub max_cashout: f64,
}

#[derive(Debug, Deserialize)]
pub struct DepositRequest {
    /// `0x`-prefixed 32-byte tx hash returned by `eth_sendRawTransaction`.
    pub tx_hash: String,
}

#[derive(Debug, Serialize)]
pub struct DepositResponse {
    pub credited_amount: f64,
    pub new_credit_balance: f64,
    pub tx_hash: String,
}

#[derive(Debug, Deserialize)]
pub struct CashoutRequest {
    /// Whole-YEET decimal amount to withdraw to the user's wallet.
    pub amount: f64,
}

#[derive(Debug, Serialize)]
pub struct CashoutResponse {
    pub withdrawal_id: Uuid,
    pub amount: f64,
    pub status: String,
    pub remaining_credit: f64,
}

#[derive(Debug, Serialize)]
pub struct CreditDepositSummary {
    pub id: Uuid,
    pub tx_hash: String,
    pub amount: f64,
    pub credited_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct CreditWithdrawalSummary {
    pub id: Uuid,
    pub amount: f64,
    pub status: String,
    pub tx_hash: Option<String>,
    pub requested_at: DateTime<Utc>,
    pub settled_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// HTTP handlers
// ---------------------------------------------------------------------------

/// Return the static public config the wallet card needs.
///
/// No auth required: every field is operator config or a constant
/// already implied by the public on-chain contract. Cached aggressively
/// by the frontend (~1 hour).
pub async fn info() -> Json<ApiResponse<CreditInfoResponse>> {
    let yeet_token_address = std::env::var("YEET_TOKEN_ADDRESS")
        .unwrap_or_else(|_| DEFAULT_YEET_TOKEN_ADDRESS.into());
    let chain_id = std::env::var("BSC_CHAIN_ID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(56);
    // Pick the RPC + explorer defaults that match `chain_id`. The
    // operator can still override `BSC_RPC_URL` explicitly for
    // private nodes / paid gateways.
    let (default_rpc, chain_name, explorer_url) = match chain_id {
        97 => (
            "https://data-seed-prebsc-1-s1.binance.org:8545/",
            "BSC Testnet",
            "https://testnet.bscscan.com",
        ),
        56 => (
            "https://bsc-dataseed.bnbchain.org",
            "BSC Mainnet",
            "https://bscscan.com",
        ),
        _ => ("", "Unknown chain", ""),
    };
    let bsc_rpc_url = std::env::var("BSC_RPC_URL").unwrap_or_else(|_| default_rpc.into());
    Json(ApiResponse::ok(CreditInfoResponse {
        deposit_address: std::env::var("YEET_DEPOSIT_ADDRESS").ok(),
        yeet_token_address,
        chain_id,
        bsc_rpc_url,
        chain_name: chain_name.into(),
        explorer_url: explorer_url.into(),
        min_cashout: MIN_CASHOUT_YEET,
        max_cashout: MAX_CASHOUT_YEET,
    }))
}

/// Verify and credit a deposit from the caller's wallet.
///
/// Idempotent on `tx_hash` — re-POSTing the same hash returns the
/// already-credited row without double-crediting. Soft-confirmed:
/// accepts any mined receipt with `status == 1`; the BSC reorg risk
/// after 1 block is low enough for v1, and the indexer (see
/// `services::indexer`) will catch up if the optimistic path missed.
///
/// # Errors
///
/// - [`AppError::Validation`] for malformed `tx_hash`, unconfirmed tx,
///   reverted tx, or a receipt with no Transfer log from the caller to
///   the deposit address.
/// - [`AppError::Internal`] if the `YEET_DEPOSIT_ADDRESS` env var is
///   not configured.
/// - [`AppError::Database`] on storage failure.
pub async fn deposit(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<DepositRequest>,
) -> AppResult<Json<ApiResponse<DepositResponse>>> {
    let (user_id, wallet_address) = caller_identity(&state, &auth).await?;

    let tx_hash = normalize_hex_hash(&req.tx_hash)?;

    let deposit_address = parse_address_env("YEET_DEPOSIT_ADDRESS")
        .ok_or_else(|| AppError::Internal("YEET_DEPOSIT_ADDRESS not configured".into()))?;

    let token_address = parse_address_env("YEET_TOKEN_ADDRESS")
        .unwrap_or_else(|| DEFAULT_YEET_TOKEN_ADDRESS.parse().expect("default addr valid"));

    let depositor: Address = wallet_address.parse().map_err(|e| {
        AppError::Validation(format!("user wallet_address is not a valid EVM address: {e}"))
    })?;

    let amount_wei = fetch_deposit_amount(&tx_hash, token_address, depositor, deposit_address).await?;
    if amount_wei.is_zero() {
        return Err(AppError::Validation(
            "Transaction does not contain a YEET transfer to the deposit address".into(),
        ));
    }
    let amount_yeet = wei_to_yeet_f64(amount_wei);

    // Insert-then-credit. `ON CONFLICT DO NOTHING` on the unique
    // `tx_hash` column makes this idempotent: a second POST with the
    // same hash returns the existing balance without re-crediting.
    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;

    let inserted: Option<(Uuid,)> = sqlx::query_as(
        "INSERT INTO credit_deposits (user_id, tx_hash, amount)
         VALUES ($1, $2, $3)
         ON CONFLICT (tx_hash) DO NOTHING
         RETURNING id",
    )
    .bind(user_id)
    .bind(&tx_hash)
    .bind(amount_yeet)
    .fetch_optional(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    if inserted.is_some() {
        sqlx::query("UPDATE users SET yeet_credit_balance = yeet_credit_balance + $1 WHERE id = $2")
            .bind(amount_yeet)
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::Database)?;
    }

    let new_balance: f64 = sqlx::query_scalar(
        "SELECT COALESCE(yeet_credit_balance, 0)::float8 FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    tx.commit().await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(DepositResponse {
        credited_amount: if inserted.is_some() { amount_yeet } else { 0.0 },
        new_credit_balance: new_balance,
        tx_hash,
    })))
}

/// Queue a cashout request for the user.
///
/// Debits the user's credit immediately (so they can't double-spend
/// while a withdrawal sits pending) and inserts a `pending` row. The
/// `services::credit_payout` worker drains pending rows on its hourly
/// tick.
///
/// # Errors
///
/// - [`AppError::Validation`] if `amount` is out of bounds or the
///   user's credit balance is insufficient.
/// - [`AppError::Database`] on storage failure.
pub async fn cashout(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CashoutRequest>,
) -> AppResult<Json<ApiResponse<CashoutResponse>>> {
    if !req.amount.is_finite() {
        return Err(AppError::Validation("Amount must be a finite number".into()));
    }
    if req.amount < MIN_CASHOUT_YEET {
        return Err(AppError::Validation(format!(
            "Minimum cashout is {MIN_CASHOUT_YEET} YEET"
        )));
    }
    if req.amount > MAX_CASHOUT_YEET {
        return Err(AppError::Validation(format!(
            "Maximum cashout is {MAX_CASHOUT_YEET} YEET; contact support for larger amounts"
        )));
    }

    let (user_id, _wallet_address) = caller_identity(&state, &auth).await?;

    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;

    let balance: f64 = sqlx::query_scalar(
        "SELECT COALESCE(yeet_credit_balance, 0)::float8 FROM users WHERE id = $1 FOR UPDATE",
    )
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    if balance < req.amount {
        return Err(AppError::Validation("Insufficient YEET".into()));
    }

    sqlx::query("UPDATE users SET yeet_credit_balance = yeet_credit_balance - $1 WHERE id = $2")
        .bind(req.amount)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

    let withdrawal_id: Uuid = sqlx::query_scalar(
        "INSERT INTO credit_withdrawals (user_id, amount, status)
         VALUES ($1, $2, 'pending')
         RETURNING id",
    )
    .bind(user_id)
    .bind(req.amount)
    .fetch_one(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    tx.commit().await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(CashoutResponse {
        withdrawal_id,
        amount: req.amount,
        status: "pending".into(),
        remaining_credit: balance - req.amount,
    })))
}

/// Recent deposits for the caller, newest first.
///
/// # Errors
/// Returns [`AppError::Database`] on storage failure.
pub async fn list_deposits(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<Vec<CreditDepositSummary>>>> {
    let (user_id, _) = caller_identity(&state, &auth).await?;

    let rows: Vec<(Uuid, String, f64, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, tx_hash, amount::float8, credited_at
           FROM credit_deposits
          WHERE user_id = $1
          ORDER BY credited_at DESC
          LIMIT $2",
    )
    .bind(user_id)
    .bind(HISTORY_LIMIT)
    .fetch_all(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    let out = rows
        .into_iter()
        .map(|r| CreditDepositSummary {
            id: r.0,
            tx_hash: r.1,
            amount: r.2,
            credited_at: r.3,
        })
        .collect();
    Ok(Json(ApiResponse::ok(out)))
}

/// Recent withdrawals for the caller, newest first.
///
/// # Errors
/// Returns [`AppError::Database`] on storage failure.
pub async fn list_withdrawals(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<Vec<CreditWithdrawalSummary>>>> {
    let (user_id, _) = caller_identity(&state, &auth).await?;

    let rows: Vec<(Uuid, f64, String, Option<String>, DateTime<Utc>, Option<DateTime<Utc>>)> =
        sqlx::query_as(
            "SELECT id, amount::float8, status, tx_hash, requested_at, settled_at
               FROM credit_withdrawals
              WHERE user_id = $1
              ORDER BY requested_at DESC
              LIMIT $2",
        )
        .bind(user_id)
        .bind(HISTORY_LIMIT)
        .fetch_all(state.db.pool())
        .await
        .map_err(AppError::Database)?;

    let out = rows
        .into_iter()
        .map(|r| CreditWithdrawalSummary {
            id: r.0,
            amount: r.1,
            status: r.2,
            tx_hash: r.3,
            requested_at: r.4,
            settled_at: r.5,
        })
        .collect();
    Ok(Json(ApiResponse::ok(out)))
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Resolve the caller's `(user_id, wallet_address)` from the JWT.
///
/// `wallet_address` is `NOT NULL` after migration 0027, so a successful
/// lookup always returns a non-empty string. Bare-bone error mapping
/// keeps it consistent with [`tips`] / [`paper_wallets`].
async fn caller_identity(state: &AppState, auth: &AuthUser) -> AppResult<(Uuid, String)> {
    if let Some(rest) = auth.address.strip_prefix("email:") {
        let user_id = Uuid::parse_str(rest)
            .map_err(|_| AppError::Validation("Invalid user id".into()))?;
        let wallet: Option<String> =
            sqlx::query_scalar("SELECT wallet_address FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_optional(state.db.pool())
                .await
                .map_err(AppError::Database)?
                .ok_or_else(|| AppError::NotFound("User not found".into()))?;
        let wallet = wallet
            .ok_or_else(|| AppError::Validation("User has no wallet_address on file".into()))?;
        return Ok((user_id, wallet));
    }
    let row: (Uuid, Option<String>) = sqlx::query_as(
        "SELECT id, wallet_address FROM users WHERE wallet_address = $1",
    )
    .bind(&auth.address)
    .fetch_optional(state.db.pool())
    .await
    .map_err(AppError::Database)?
    .ok_or_else(|| AppError::NotFound("User not found".into()))?;
    let wallet =
        row.1.ok_or_else(|| AppError::Validation("User has no wallet_address on file".into()))?;
    Ok((row.0, wallet))
}

/// Normalize a hex tx hash: lowercase, `0x`-prefixed, 64 hex chars after
/// the prefix. Returns the canonical string.
fn normalize_hex_hash(input: &str) -> AppResult<String> {
    let trimmed = input.trim();
    let body = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    if body.len() != 64 || !body.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::Validation(
            "tx_hash must be a 0x-prefixed 32-byte hex string".into(),
        ));
    }
    Ok(format!("0x{}", body.to_ascii_lowercase()))
}

/// Parse an EVM address from the named env var. Returns `None` when
/// unset or unparseable (caller decides whether that's fatal).
fn parse_address_env(var: &str) -> Option<Address> {
    let value = std::env::var(var).ok()?;
    value.parse().ok()
}

/// Fetch the BSC transaction receipt and sum any YEET Transfer events
/// from `depositor` to `deposit_to`. Returns 0 when no matching log is
/// present; bubbles up RPC errors as [`AppError::Validation`] (the
/// caller's tx hash is treated as user input).
async fn fetch_deposit_amount(
    tx_hash: &str,
    token: Address,
    depositor: Address,
    deposit_to: Address,
) -> AppResult<U256> {
    let rpc_url = std::env::var("BSC_RPC_URL").unwrap_or_else(|_| DEFAULT_BSC_RPC.into());
    let provider = Provider::<Http>::try_from(&rpc_url)
        .map_err(|e| AppError::Internal(format!("invalid BSC_RPC_URL: {e}")))?;

    let hash: H256 = tx_hash
        .parse()
        .map_err(|e| {
            AppError::Validation(format!("tx_hash parse failure: {e}"))
        })?;

    let receipt = provider
        .get_transaction_receipt(hash)
        .await
        .map_err(|e| AppError::Validation(format!("RPC error fetching receipt: {e}")))?
        .ok_or_else(|| AppError::Validation("Transaction not yet confirmed".into()))?;

    if receipt.status != Some(U64::from(1)) {
        return Err(AppError::Validation(
            "Transaction reverted on-chain".into(),
        ));
    }

    let transfer_topic: H256 = TRANSFER_TOPIC
        .parse()
        .map_err(|e| {
            AppError::Internal(format!("transfer topic constant invalid: {e}"))
        })?;

    let mut sum = U256::zero();
    for log in &receipt.logs {
        if log.address != token {
            continue;
        }
        if log.topics.len() < 3 || log.topics[0] != transfer_topic {
            continue;
        }
        if topic_to_address(&log.topics[1]) != depositor {
            continue;
        }
        if topic_to_address(&log.topics[2]) != deposit_to {
            continue;
        }
        if log.data.len() >= 32 {
            sum = sum.saturating_add(U256::from_big_endian(&log.data[..32]));
        }
    }
    Ok(sum)
}

/// Convert a 32-byte topic (left-padded address) into a 20-byte `Address`.
fn topic_to_address(topic: &H256) -> Address {
    let bytes = topic.as_bytes();
    Address::from_slice(&bytes[12..32])
}

/// Convert wei to `f64` YEET. Precision-lossy for amounts > ~9 PYEET,
/// fine for everyday balances. Higher-fidelity callers should hold
/// `U256` directly.
fn wei_to_yeet_f64(wei: U256) -> f64 {
    let denom = U256::exp10(YEET_DECIMALS as usize);
    let whole = wei / denom;
    let frac = wei % denom;
    let whole_f = whole.as_u128() as f64;
    let frac_f = frac.as_u128() as f64 / 1e18_f64;
    whole_f + frac_f
}

// Rust guideline compliant 2026-02-21
