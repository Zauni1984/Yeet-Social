//! Admin-only transaction-ledger API: query, CSV export, tax summary and a
//! tamper-evidence (hash-chain) check. Access is gated on the admin secret and
//! only reachable through the backend — there is no user-facing surface.
use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::services::ledger;

fn check_admin(secret: &str) -> AppResult<()> {
    let admin_secret = std::env::var("ADMIN_SECRET").unwrap_or_else(|_| "yeet_admin_2024".to_string());
    if secret != admin_secret {
        return Err(AppError::Unauthorised("Invalid admin secret".into()));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct LedgerQuery {
    pub secret: Option<String>,
    pub from: Option<chrono::DateTime<chrono::Utc>>,
    pub to: Option<chrono::DateTime<chrono::Utc>>,
    pub tx_type: Option<String>,
    pub asset: Option<String>,
    pub user_id: Option<uuid::Uuid>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct LedgerRow {
    pub entry_no: i64,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
    pub tx_type: String,
    pub asset: String,
    pub amount: String,       // exact decimal as text (tax precision)
    pub fee_amount: String,
    pub user_id: Option<uuid::Uuid>,
    pub counterparty_id: Option<uuid::Uuid>,
    pub user_wallet: Option<String>,
    pub counterparty_wallet: Option<String>,
    pub reference_type: Option<String>,
    pub reference_id: Option<String>,
    pub onchain_tx_hash: Option<String>,
    pub fiat_currency: String,
    pub fiat_value: Option<String>,
    pub fx_rate: Option<String>,
    pub fx_source: Option<String>,
    pub description: Option<String>,
    pub created_by: String,
    pub prev_hash: String,
    pub entry_hash: String,
}

const SELECT_COLS: &str = "entry_no, occurred_at, recorded_at, tx_type, asset,
    amount::text AS amount, fee_amount::text AS fee_amount,
    user_id, counterparty_id, user_wallet, counterparty_wallet,
    reference_type, reference_id, onchain_tx_hash,
    fiat_currency, fiat_value::text AS fiat_value, fx_rate::text AS fx_rate, fx_source,
    description, created_by, prev_hash, entry_hash";

// All filters are optional and applied via NULL-guards so we never build SQL
// by string concatenation. Params: $1 from, $2 to, $3 tx_type, $4 asset,
// $5 user_id (matches either side).
const WHERE_FILTERS: &str = "
    ($1::timestamptz IS NULL OR occurred_at >= $1)
    AND ($2::timestamptz IS NULL OR occurred_at < $2)
    AND ($3::text IS NULL OR tx_type = $3)
    AND ($4::text IS NULL OR asset = $4)
    AND ($5::uuid IS NULL OR user_id = $5 OR counterparty_id = $5)";

async fn fetch_rows(state: &AppState, q: &LedgerQuery, limit: i64, offset: i64) -> AppResult<Vec<LedgerRow>> {
    let sql = format!(
        "SELECT {SELECT_COLS} FROM ledger_entries
          WHERE {WHERE_FILTERS}
          ORDER BY entry_no ASC
          LIMIT $6 OFFSET $7"
    );
    sqlx::query_as::<_, LedgerRow>(&sql)
        .bind(q.from).bind(q.to)
        .bind(q.tx_type.as_deref()).bind(q.asset.as_deref()).bind(q.user_id)
        .bind(limit).bind(offset)
        .fetch_all(state.db.pool()).await.map_err(AppError::Database)
}

/// GET /api/v1/admin/ledger — paginated JSON list with filters.
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<LedgerQuery>,
) -> AppResult<Json<ApiResponse<Vec<LedgerRow>>>> {
    check_admin(q.secret.as_deref().unwrap_or(""))?;
    let per_page = q.per_page.unwrap_or(100).clamp(1, 1000);
    let page = q.page.unwrap_or(1).max(1);
    let rows = fetch_rows(&state, &q, per_page, (page - 1) * per_page).await?;
    Ok(Json(ApiResponse::ok(rows)))
}

fn csv_field(s: &str) -> String {
    // Quote + escape for CSV; always quote to keep decimals/dates intact in
    // German Excel (semicolon-separated) and DATEV imports.
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// GET /api/v1/admin/ledger/export — full CSV export (Finanzamt/DATEV friendly).
/// Semicolon-delimited + UTF-8 BOM so it opens cleanly in German Excel.
pub async fn export_csv(
    State(state): State<AppState>,
    Query(q): Query<LedgerQuery>,
) -> Result<Response, AppError> {
    check_admin(q.secret.as_deref().unwrap_or(""))?;

    // Export can be large; page through in chunks to bound memory.
    let mut out = String::new();
    out.push('\u{FEFF}'); // BOM
    out.push_str("entry_no;occurred_at;recorded_at;tx_type;asset;amount;fee_amount;\
                  user_id;counterparty_id;user_wallet;counterparty_wallet;\
                  reference_type;reference_id;onchain_tx_hash;\
                  fiat_currency;fiat_value;fx_rate;fx_source;description;created_by;\
                  prev_hash;entry_hash\n");

    let chunk = 5000i64;
    let mut offset = 0i64;
    loop {
        let rows = fetch_rows(&state, &q, chunk, offset).await?;
        if rows.is_empty() { break; }
        for r in &rows {
            let cols = [
                r.entry_no.to_string(),
                r.occurred_at.to_rfc3339(),
                r.recorded_at.to_rfc3339(),
                r.tx_type.clone(),
                r.asset.clone(),
                r.amount.clone(),
                r.fee_amount.clone(),
                r.user_id.map(|v| v.to_string()).unwrap_or_default(),
                r.counterparty_id.map(|v| v.to_string()).unwrap_or_default(),
                r.user_wallet.clone().unwrap_or_default(),
                r.counterparty_wallet.clone().unwrap_or_default(),
                r.reference_type.clone().unwrap_or_default(),
                r.reference_id.clone().unwrap_or_default(),
                r.onchain_tx_hash.clone().unwrap_or_default(),
                r.fiat_currency.clone(),
                r.fiat_value.clone().unwrap_or_default(),
                r.fx_rate.clone().unwrap_or_default(),
                r.fx_source.clone().unwrap_or_default(),
                r.description.clone().unwrap_or_default(),
                r.created_by.clone(),
                r.prev_hash.clone(),
                r.entry_hash.clone(),
            ];
            let line: Vec<String> = cols.iter().map(|c| csv_field(c)).collect();
            out.push_str(&line.join(";"));
            out.push('\n');
        }
        if (rows.len() as i64) < chunk { break; }
        offset += chunk;
    }

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/csv; charset=utf-8"));
    headers.insert(header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"yeet_ledger.csv\""));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok((StatusCode::OK, headers, out).into_response())
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SummaryRow {
    pub tx_type: String,
    pub asset: String,
    pub entries: i64,
    pub total_credit: String,
    pub total_debit: String,
    pub net: String,
    pub total_fee: String,
}

/// GET /api/v1/admin/ledger/summary — aggregates per tx_type + asset for the
/// selected period (the shape a tax advisor / Finanzamt export wants).
pub async fn summary(
    State(state): State<AppState>,
    Query(q): Query<LedgerQuery>,
) -> AppResult<Json<ApiResponse<Vec<SummaryRow>>>> {
    check_admin(q.secret.as_deref().unwrap_or(""))?;
    let sql = format!(
        "SELECT tx_type, asset,
                COUNT(*)::bigint AS entries,
                COALESCE(SUM(amount) FILTER (WHERE amount > 0), 0)::text AS total_credit,
                COALESCE(SUM(-amount) FILTER (WHERE amount < 0), 0)::text AS total_debit,
                COALESCE(SUM(amount), 0)::text AS net,
                COALESCE(SUM(fee_amount), 0)::text AS total_fee
           FROM ledger_entries
          WHERE {WHERE_FILTERS}
          GROUP BY tx_type, asset
          ORDER BY asset, tx_type"
    );
    let rows = sqlx::query_as::<_, SummaryRow>(&sql)
        .bind(q.from).bind(q.to)
        .bind(q.tx_type.as_deref()).bind(q.asset.as_deref()).bind(q.user_id)
        .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(rows)))
}

#[derive(Debug, Serialize)]
pub struct VerifyResponse {
    pub intact: bool,
    /// entry_no where the chain first breaks (null if intact).
    pub broken_at: Option<i64>,
    pub total_entries: i64,
}

/// GET /api/v1/admin/ledger/verify — recompute the hash chain and report
/// whether the ledger has been tampered with (evidence integrity).
pub async fn verify(
    State(state): State<AppState>,
    Query(q): Query<LedgerQuery>,
) -> AppResult<Json<ApiResponse<VerifyResponse>>> {
    check_admin(q.secret.as_deref().unwrap_or(""))?;
    let broken_at = ledger::verify_chain(state.db.pool()).await?;
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*)::bigint FROM ledger_entries")
        .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(VerifyResponse {
        intact: broken_at.is_none(),
        broken_at,
        total_entries: total,
    })))
}
