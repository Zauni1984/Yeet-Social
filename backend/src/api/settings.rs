//! User settings handlers — currency preference, display options, notifications.
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, api::middleware::AuthUser, models::ApiResponse};

async fn resolve_user_id(state: &AppState, auth_address: &str) -> AppResult<Uuid> {
    if let Some(uuid_str) = auth_address.strip_prefix("email:") {
        return uuid_str.parse::<Uuid>().map_err(|_| AppError::NotFound("Invalid user ID".into()));
    }
    sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
        .bind(auth_address)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct UserSettings {
    pub currency:              String,    // "USD" | "EUR" | "GBP" etc.
    pub language:              String,    // "en" | "de" | "fr" etc.
    pub show_nsfw:             bool,
    pub email_notifications:   bool,
    pub push_notifications:    bool,
    pub auto_play_media:       bool,
    pub compact_mode:          bool,
    /// Feed filter: only posts in these languages (ISO 639-1). Empty = all.
    #[serde(default)]
    #[sqlx(default)]
    pub feed_langs:            Vec<String>,
    /// Feed filter: only posts by authors from these countries (ISO 3166-1 alpha-2). Empty = all.
    #[serde(default)]
    #[sqlx(default)]
    pub feed_countries:        Vec<String>,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            currency:            "USD".into(),
            language:            "en".into(),
            show_nsfw:           false,
            email_notifications: true,
            push_notifications:  true,
            auto_play_media:     true,
            compact_mode:        false,
            feed_langs:          Vec::new(),
            feed_countries:      Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateSettingsRequest {
    pub currency:              Option<String>,
    pub language:              Option<String>,
    pub show_nsfw:             Option<bool>,
    pub email_notifications:   Option<bool>,
    pub push_notifications:    Option<bool>,
    pub auto_play_media:       Option<bool>,
    pub compact_mode:          Option<bool>,
    pub feed_langs:            Option<Vec<String>>,
    pub feed_countries:        Option<Vec<String>>,
}

/// Normalise a feed filter list: lower/upper-case, dedupe, drop junk,
/// cap the length. Languages must be 2 lower-case letters, countries 2
/// upper-case letters.
fn clean_codes(list: &[String], upper: bool, max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in list {
        let c = raw.trim();
        let c = if upper { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() };
        if c.len() == 2 && c.chars().all(|ch| ch.is_ascii_alphabetic()) && !out.contains(&c) { out.push(c); }
        if out.len() >= max { break; }
    }
    out
}

const SUPPORTED_CURRENCIES: &[&str] = &["USD", "EUR", "GBP", "CHF", "JPY", "BTC", "ETH", "BNB"];

pub async fn get_settings(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<UserSettings>>> {
    let user_id = resolve_user_id(&state, &auth.address).await?;
    let settings = sqlx::query_as::<_, UserSettings>(
        "SELECT currency, language, show_nsfw, email_notifications,
                push_notifications, auto_play_media, compact_mode
         FROM user_settings WHERE user_id = $1, feed_langs, feed_countries"
    )
    .bind(user_id)
    .fetch_optional(state.db.pool())
    .await
    .map_err(AppError::Database)?
    .unwrap_or_default();

    Ok(Json(ApiResponse::ok(settings)))
}

pub async fn update_settings(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<UpdateSettingsRequest>,
) -> AppResult<Json<ApiResponse<UserSettings>>> {
    // Validate currency
    if let Some(ref cur) = req.currency {
        if !SUPPORTED_CURRENCIES.contains(&cur.as_str()) {
            return Err(AppError::Validation(
                format!("Unsupported currency. Supported: {}", SUPPORTED_CURRENCIES.join(", "))
            ));
        }
    }

    let user_id = resolve_user_id(&state, &auth.address).await?;
    let feed_langs = req.feed_langs.as_deref().map(|l| clean_codes(l, false, 20));
    let feed_countries = req.feed_countries.as_deref().map(|l| clean_codes(l, true, 50));
    // Upsert settings
    sqlx::query(
        "INSERT INTO user_settings
            (user_id, currency, language, show_nsfw,
             email_notifications, push_notifications, auto_play_media, compact_mode,
             feed_langs, feed_countries)
         VALUES ($1,
             COALESCE($2, 'USD'), COALESCE($3, 'en'), COALESCE($4, false),
             COALESCE($5, true),  COALESCE($6, true), COALESCE($7, true),  COALESCE($8, false),
             COALESCE($9, '{}'), COALESCE($10, '{}'))
         ON CONFLICT (user_id) DO UPDATE SET
             currency            = COALESCE($2,  user_settings.currency),
             language            = COALESCE($3,  user_settings.language),
             show_nsfw           = COALESCE($4,  user_settings.show_nsfw),
             email_notifications = COALESCE($5,  user_settings.email_notifications),
             push_notifications  = COALESCE($6,  user_settings.push_notifications),
             auto_play_media     = COALESCE($7,  user_settings.auto_play_media),
             compact_mode        = COALESCE($8,  user_settings.compact_mode),
             feed_langs          = COALESCE($9,  user_settings.feed_langs),
             feed_countries      = COALESCE($10, user_settings.feed_countries),
             updated_at          = NOW()"
    )
    .bind(user_id)
    .bind(&req.currency)
    .bind(&req.language)
    .bind(req.show_nsfw)
    .bind(req.email_notifications)
    .bind(req.push_notifications)
    .bind(req.auto_play_media)
    .bind(req.compact_mode)
    .bind(&feed_langs)
    .bind(&feed_countries)
    .execute(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    get_settings(State(state), auth).await
}