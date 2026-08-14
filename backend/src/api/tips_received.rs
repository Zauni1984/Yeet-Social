//! Per-user "tips received" overview.
//!
//! Strictly private: the handlers resolve the *logged-in* user from the
//! auth token and only ever return the tips that user received — there is
//! no path to read someone else's history. Sortable by sender name / tip
//! amount / date, with a German-Excel-friendly CSV export (BOM + `;`).
use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;

async fn resolve_user_id(state: &AppState, auth_address: &str) -> AppResult<Uuid> {
    if let Some(uuid_str) = auth_address.strip_prefix("email:") {
        return uuid_str.parse::<Uuid>()
            .map_err(|_| AppError::NotFound("Invalid user ID".into()));
    }
    sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
        .bind(auth_address)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))
}

#[derive(Debug, Deserialize)]
pub struct TipsQuery {
    /// name | amount | date  (default: date)
    pub sort: Option<String>,
    /// asc | desc            (default: desc)
    pub order: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TipReceived {
    pub id: Uuid,
    pub from_user_id: Uuid,
    pub from_username: Option<String>,
    pub from_display_name: Option<String>,
    pub from_wallet: Option<String>,
    /// creator_amount — what actually landed after the platform cut.
    pub amount_received: f64,
    /// original gross tip amount.
    pub gross_amount: f64,
    pub currency: String,
    pub post_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// Build a safe ORDER BY from a fixed whitelist (never interpolate raw input).
fn order_by(sort: Option<&str>, order: Option<&str>) -> String {
    let dir = if matches!(order, Some("asc")) { "ASC" } else { "DESC" };
    let col = match sort {
        Some("name")   => "sender_name",
        Some("amount") => "t.creator_amount",
        _              => "t.created_at",
    };
    format!("ORDER BY {col} {dir}, t.created_at DESC")
}

async fn fetch(state: &AppState, uid: Uuid, q: &TipsQuery) -> AppResult<Vec<TipReceived>> {
    let sql = format!(
        "SELECT t.id,
                t.from_user_id,
                u.username        AS from_username,
                u.display_name    AS from_display_name,
                u.wallet_address  AS from_wallet,
                COALESCE(u.display_name, u.username, u.wallet_address, '') AS sender_name,
                t.creator_amount::float8 AS amount_received,
                t.amount::float8         AS gross_amount,
                t.currency::text         AS currency,
                t.post_id,
                t.created_at
           FROM tips t
           JOIN users u ON u.id = t.from_user_id
          WHERE t.to_user_id = $1
          {}",
        order_by(q.sort.as_deref(), q.order.as_deref())
    );
    sqlx::query_as::<_, TipReceived>(&sql)
        .bind(uid)
        .fetch_all(state.db.pool()).await.map_err(AppError::Database)
}

/// GET /api/v1/me/tips-received?sort=&order=
pub async fn list_mine(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<TipsQuery>,
) -> AppResult<Json<ApiResponse<Vec<TipReceived>>>> {
    let uid = resolve_user_id(&state, &auth.address).await?;
    let rows = fetch(&state, uid, &q).await?;
    Ok(Json(ApiResponse::ok(rows)))
}

fn csv_field(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// GET /api/v1/me/tips-received/export — CSV of the caller's received tips.
/// Semicolon-delimited + UTF-8 BOM so it opens cleanly in German Excel.
pub async fn export_csv(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<TipsQuery>,
) -> Result<Response, AppError> {
    let uid = resolve_user_id(&state, &auth.address).await?;
    let rows = fetch(&state, uid, &q).await?;

    let mut out = String::new();
    out.push('\u{FEFF}'); // BOM
    out.push_str("date;from_name;from_username;from_wallet;amount_received;gross_amount;currency;post_id\n");
    for r in &rows {
        let name = r.from_display_name.clone()
            .or_else(|| r.from_username.clone())
            .or_else(|| r.from_wallet.clone())
            .unwrap_or_default();
        let cols = [
            r.created_at.to_rfc3339(),
            name,
            r.from_username.clone().unwrap_or_default(),
            r.from_wallet.clone().unwrap_or_default(),
            format!("{:.8}", r.amount_received),
            format!("{:.8}", r.gross_amount),
            r.currency.clone(),
            r.post_id.map(|v| v.to_string()).unwrap_or_default(),
        ];
        let line: Vec<String> = cols.iter().map(|c| csv_field(c)).collect();
        out.push_str(&line.join(";"));
        out.push('\n');
    }

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/csv; charset=utf-8"));
    headers.insert(header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"my_tips.csv\""));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok((StatusCode::OK, headers, out).into_response())
}
