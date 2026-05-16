//! Paper Wallet handlers.
//!
//! A paper wallet is a custodial claim ticket: at issuance the issuer's
//! `yeet_token_balance` is debited and the amount is locked against a
//! hashed secret. Whoever later submits the secret receives the credit.
//! The issuer can void an unclaimed bill to refund the balance.

use axum::{extract::{Path, State}, Json};
use chrono::{DateTime, Utc};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;

const SECRET_BYTES: usize = 24; // 192 bits → 39-char base32 (no padding)

// ---------- helpers ----------

async fn caller_user_id(state: &AppState, auth: &AuthUser) -> AppResult<Uuid> {
    if let Some(rest) = auth.address.strip_prefix("email:") {
        return Uuid::parse_str(rest)
            .map_err(|_| AppError::Validation("Invalid user id".into()));
    }
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE wallet_address = $1")
        .bind(&auth.address)
        .fetch_optional(state.db.pool())
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))
}

/// Crockford base32 — unambiguous (no I, L, O, U), case-insensitive.
fn base32_crockford(bytes: &[u8]) -> String {
    const ALPH: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut out = String::with_capacity((bytes.len() * 8).div_ceil(5));
    let (mut buf, mut bits) = (0u32, 0u32);
    for &b in bytes {
        buf = (buf << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPH[((buf >> bits) & 0x1f) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPH[((buf << (5 - bits)) & 0x1f) as usize] as char);
    }
    out
}

/// Parse Crockford base32; tolerates lowercase and the common confusable
/// substitutions (I/L → 1, O → 0).
fn base32_crockford_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 5 / 8);
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.chars() {
        if c == '-' || c == ' ' { continue; }
        let v: u32 = match c.to_ascii_uppercase() {
            '0' | 'O' => 0,
            '1' | 'I' | 'L' => 1,
            c @ '2'..='9' => c as u32 - '0' as u32,
            c @ 'A'..='H' => c as u32 - 'A' as u32 + 10,
            'J' => 18, 'K' => 19, 'M' => 20, 'N' => 21,
            c @ 'P'..='T' => c as u32 - 'P' as u32 + 22,
            'V' => 27, 'W' => 28, 'X' => 29, 'Y' => 30, 'Z' => 31,
            _ => return None,
        };
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

fn sha256(input: &[u8]) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(input);
    h.finalize().to_vec()
}

fn make_serial(rng: &mut impl RngCore) -> String {
    // 8 random bytes → 13-char base32 → grouped as XXXX-XXXX-XXXX-X for readability
    let mut b = [0u8; 8];
    rng.fill_bytes(&mut b);
    let s = base32_crockford(&b);
    // group every 4 chars
    let mut out = String::with_capacity(s.len() + 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && i % 4 == 0 { out.push('-'); }
        out.push(c);
    }
    format!("YEET-{}", out)
}

// ---------- DTOs ----------

#[derive(Debug, Deserialize)]
pub struct CreatePaperWalletRequest {
    pub amount: f64,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreatePaperWalletResponse {
    pub id: Uuid,
    pub serial: String,
    pub amount: f64,
    pub currency: String,
    pub claim_secret: String,   // base32 — shown ONCE, encoded into the QR
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct PaperWalletSummary {
    pub id: Uuid,
    pub serial: String,
    pub amount: f64,
    pub currency: String,
    pub status: String,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub voided_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct RedeemRequest {
    pub secret: String,
}

#[derive(Debug, Serialize)]
pub struct RedeemResponse {
    pub serial: String,
    pub amount: f64,
    pub currency: String,
}

// ---------- handlers ----------

pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreatePaperWalletRequest>,
) -> AppResult<Json<ApiResponse<CreatePaperWalletResponse>>> {
    if !req.amount.is_finite() || req.amount <= 0.0 {
        return Err(AppError::Validation("Amount must be greater than 0".into()));
    }
    if req.amount > 1_000_000.0 {
        return Err(AppError::Validation("Amount too large".into()));
    }
    if let Some(n) = &req.note {
        if n.chars().count() > 80 {
            return Err(AppError::Validation("Note too long (max 80 chars)".into()));
        }
    }

    let issuer_id = caller_user_id(&state, &auth).await?;

    // Atomic debit + insert in a single transaction.
    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;

    // Lock the row to prevent concurrent withdrawals
    let balance: f64 = sqlx::query_scalar(
        "SELECT COALESCE(yeet_token_balance, 0)::float8 FROM users WHERE id = $1 FOR UPDATE"
    )
    .bind(issuer_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    if balance < req.amount {
        return Err(AppError::Validation("Insufficient YEET balance".into()));
    }

    // Generate secret + serial up-front (RNG held only synchronously, so
    // we don't drag a `!Send` value across await points).
    let mut secret_bytes = vec![0u8; SECRET_BYTES];
    OsRng.fill_bytes(&mut secret_bytes);
    let secret_str = base32_crockford(&secret_bytes);
    let secret_hash = sha256(secret_str.as_bytes());

    // Pre-generate a small pool of candidate serials so we never hold the
    // RNG across an await.
    let mut candidates: Vec<String> = (0..5).map(|_| make_serial(&mut OsRng)).collect();
    let mut serial = candidates.remove(0);
    while !candidates.is_empty() {
        let exists: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM paper_wallets WHERE serial = $1"
        )
        .bind(&serial)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::Database)?;
        if exists.is_none() { break; }
        serial = candidates.remove(0);
    }

    let row: (Uuid, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO paper_wallets (issuer_id, amount, currency, serial, claim_secret_hash, note)
         VALUES ($1, $2, 'YEET', $3, $4, $5)
         RETURNING id, created_at"
    )
    .bind(issuer_id)
    .bind(req.amount)
    .bind(&serial)
    .bind(&secret_hash)
    .bind(req.note.as_deref())
    .fetch_one(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    sqlx::query("UPDATE users SET yeet_token_balance = yeet_token_balance - $1 WHERE id = $2")
        .bind(req.amount)
        .bind(issuer_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

    tx.commit().await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(CreatePaperWalletResponse {
        id: row.0,
        serial,
        amount: req.amount,
        currency: "YEET".into(),
        claim_secret: secret_str,
        created_at: row.1,
    })))
}

pub async fn list_mine(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<Vec<PaperWalletSummary>>>> {
    let issuer_id = caller_user_id(&state, &auth).await?;

    let rows: Vec<(Uuid, String, f64, String, String, Option<String>,
                   DateTime<Utc>, Option<DateTime<Utc>>, Option<DateTime<Utc>>)> =
        sqlx::query_as(
            "SELECT id, serial, amount::float8, currency, status, note,
                    created_at, claimed_at, voided_at
               FROM paper_wallets
              WHERE issuer_id = $1
              ORDER BY created_at DESC
              LIMIT 200"
        )
        .bind(issuer_id)
        .fetch_all(state.db.pool())
        .await
        .map_err(AppError::Database)?;

    let out = rows.into_iter().map(|r| PaperWalletSummary {
        id: r.0, serial: r.1, amount: r.2, currency: r.3, status: r.4,
        note: r.5, created_at: r.6, claimed_at: r.7, voided_at: r.8,
    }).collect();

    Ok(Json(ApiResponse::ok(out)))
}

pub async fn redeem(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<RedeemRequest>,
) -> AppResult<Json<ApiResponse<RedeemResponse>>> {
    let trimmed: String = req.secret.chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    if base32_crockford_decode(&trimmed).is_none() {
        return Err(AppError::Validation("Invalid claim code format".into()));
    }
    let secret_hash = sha256(trimmed.to_uppercase().as_bytes());

    let claimer_id = caller_user_id(&state, &auth).await?;

    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;

    // Atomic: claim only if still active, return the row data
    let claimed: Option<(Uuid, Uuid, f64, String, String)> = sqlx::query_as(
        "UPDATE paper_wallets
            SET status = 'claimed', claimed_by_id = $1, claimed_at = NOW()
          WHERE claim_secret_hash = $2 AND status = 'active'
          RETURNING id, issuer_id, amount::float8, currency, serial"
    )
    .bind(claimer_id)
    .bind(&secret_hash)
    .fetch_optional(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    let (_id, issuer_id, amount, currency, serial) = claimed.ok_or_else(||
        AppError::NotFound("Claim code is invalid, already used or voided".into()))?;

    if issuer_id == claimer_id {
        // Self-claim is a no-op refund — disallow to avoid griefing & keep
        // semantics clean. Issuers should void instead.
        return Err(AppError::Validation(
            "You can't redeem your own paper wallet — void it instead".into()));
    }

    sqlx::query("UPDATE users SET yeet_token_balance = yeet_token_balance + $1 WHERE id = $2")
        .bind(amount)
        .bind(claimer_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

    tx.commit().await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(RedeemResponse { serial, amount, currency })))
}

pub async fn void(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<PaperWalletSummary>>> {
    let issuer_id = caller_user_id(&state, &auth).await?;

    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;

    let voided: Option<(Uuid, String, f64, String, String, Option<String>,
                        DateTime<Utc>, Option<DateTime<Utc>>, Option<DateTime<Utc>>)> =
        sqlx::query_as(
            "UPDATE paper_wallets
                SET status = 'voided', voided_at = NOW()
              WHERE id = $1 AND issuer_id = $2 AND status = 'active'
              RETURNING id, serial, amount::float8, currency, status, note,
                        created_at, claimed_at, voided_at"
        )
        .bind(id)
        .bind(issuer_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::Database)?;

    let r = voided.ok_or_else(|| AppError::NotFound(
        "Paper wallet not found, not yours, or no longer active".into()))?;

    sqlx::query("UPDATE users SET yeet_token_balance = yeet_token_balance + $1 WHERE id = $2")
        .bind(r.2)
        .bind(issuer_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

    tx.commit().await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(PaperWalletSummary {
        id: r.0, serial: r.1, amount: r.2, currency: r.3, status: r.4,
        note: r.5, created_at: r.6, claimed_at: r.7, voided_at: r.8,
    })))
}
