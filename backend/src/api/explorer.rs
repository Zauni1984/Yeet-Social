//! Public YEET token explorer API (read-only) — token info, richlist,
//! per-address balances and transfer history. Intended for third-party
//! providers, so responses are plain public JSON. Data is served from the
//! indexer tables (migration 0040) and is empty until the token is deployed
//! and the indexer is running.
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use crate::{AppError, AppResult, AppState, models::ApiResponse};

fn is_addr(s: &str) -> bool {
    s.len() == 42 && s.starts_with("0x") && s[2..].chars().all(|c| c.is_ascii_hexdigit())
}

#[derive(Debug, Serialize)]
pub struct TokenInfo {
    pub name: &'static str,
    pub symbol: &'static str,
    pub decimals: u8,
    pub chain_id: u64,
    pub contract: Option<String>,
    pub holder_count: i64,
    pub transfer_count: i64,
    pub circulating_wei: String,
    pub last_indexed_block: i64,
}

/// GET /api/v1/explorer/token — token metadata + aggregate stats.
pub async fn token_info(State(state): State<AppState>) -> AppResult<Json<ApiResponse<TokenInfo>>> {
    let chain_id: u64 = std::env::var("YEET_CHAIN_ID").ok()
        .and_then(|s| s.parse().ok()).unwrap_or(56);
    let contract = std::env::var("YEET_TOKEN_ADDRESS").ok()
        .filter(|a| a != "0x0000000000000000000000000000000000000000" && !a.is_empty());

    let stats: Option<(i64, i64, String, i64)> = sqlx::query_as(
        "SELECT holder_count, transfer_count, circulating::text, last_indexed_block
           FROM token_stats WHERE id = 1"
    )
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (holder_count, transfer_count, circulating, last_block) =
        stats.unwrap_or((0, 0, "0".to_string(), 0));

    Ok(Json(ApiResponse::ok(TokenInfo {
        name: "Yeet Token", symbol: "YEET", decimals: 18, chain_id, contract,
        holder_count, transfer_count, circulating_wei: circulating,
        last_indexed_block: last_block,
    })))
}

#[derive(Debug, Deserialize)]
pub struct RichlistQuery { pub limit: Option<i64>, pub offset: Option<i64> }

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct HolderRow {
    pub rank: i64,
    pub address: String,
    pub balance_wei: String,
    pub tx_count: i64,
}

/// GET /api/v1/explorer/richlist?limit=100&offset=0 — top holders by balance.
pub async fn richlist(
    State(state): State<AppState>,
    Query(q): Query<RichlistQuery>,
) -> AppResult<Json<ApiResponse<Vec<HolderRow>>>> {
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);
    let offset = q.offset.unwrap_or(0).max(0);
    let rows = sqlx::query_as::<_, HolderRow>(
        "SELECT (ROW_NUMBER() OVER (ORDER BY balance DESC) + $2)::bigint AS rank,
                address, balance::text AS balance_wei, tx_count
           FROM token_holders
          WHERE balance > 0
          ORDER BY balance DESC
          LIMIT $1 OFFSET $2"
    )
    .bind(limit).bind(offset)
    .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(rows)))
}

#[derive(Debug, Serialize)]
pub struct HolderDetail {
    pub address: String,
    pub balance_wei: String,
    pub tx_count: i64,
    pub rank: Option<i64>,
}

/// GET /api/v1/explorer/holders/:address — one holder's balance + rank.
pub async fn holder(
    State(state): State<AppState>,
    Path(address): Path<String>,
) -> AppResult<Json<ApiResponse<HolderDetail>>> {
    let address = address.to_lowercase();
    if !is_addr(&address) {
        return Err(AppError::Validation("Invalid address".into()));
    }
    let row: Option<(String, i64)> = sqlx::query_as(
        "SELECT balance::text, tx_count FROM token_holders WHERE address = $1"
    )
    .bind(&address)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;

    let (balance_wei, tx_count) = row.unwrap_or(("0".to_string(), 0));
    let rank: Option<i64> = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint + 1 FROM token_holders
          WHERE balance > (SELECT COALESCE(balance,0) FROM token_holders WHERE address = $1)
            AND balance > 0"
    )
    .bind(&address)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(HolderDetail { address, balance_wei, tx_count, rank })))
}

#[derive(Debug, Deserialize)]
pub struct TransfersQuery {
    pub address: Option<String>,
    pub limit: Option<i64>,
    pub before_block: Option<i64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TransferRow {
    pub tx_hash: String,
    pub log_index: i32,
    pub block_number: i64,
    pub block_time: Option<chrono::DateTime<chrono::Utc>>,
    pub from_address: String,
    pub to_address: String,
    pub value_wei: String,
}

/// GET /api/v1/explorer/transfers?address=&limit=&before_block= — transfer feed
/// (optionally filtered to one address, either side), newest first.
pub async fn transfers(
    State(state): State<AppState>,
    Query(q): Query<TransfersQuery>,
) -> AppResult<Json<ApiResponse<Vec<TransferRow>>>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let addr = q.address.map(|a| a.to_lowercase());
    if let Some(a) = &addr {
        if !is_addr(a) { return Err(AppError::Validation("Invalid address".into())); }
    }
    let before = q.before_block.unwrap_or(i64::MAX);
    let rows = sqlx::query_as::<_, TransferRow>(
        "SELECT tx_hash, log_index, block_number, block_time,
                from_address, to_address, value::text AS value_wei
           FROM token_transfers
          WHERE block_number < $1
            AND ($2::text IS NULL OR from_address = $2 OR to_address = $2)
          ORDER BY block_number DESC, log_index DESC
          LIMIT $3"
    )
    .bind(before).bind(addr.as_deref()).bind(limit)
    .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(rows)))
}

/// GET /api/v1/explorer/tx/:hash — all token transfers in one transaction.
pub async fn tx_transfers(
    State(state): State<AppState>,
    Path(hash): Path<String>,
) -> AppResult<Json<ApiResponse<Vec<TransferRow>>>> {
    let hash = hash.to_lowercase();
    let rows = sqlx::query_as::<_, TransferRow>(
        "SELECT tx_hash, log_index, block_number, block_time,
                from_address, to_address, value::text AS value_wei
           FROM token_transfers WHERE tx_hash = $1 ORDER BY log_index ASC"
    )
    .bind(&hash)
    .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(rows)))
}
