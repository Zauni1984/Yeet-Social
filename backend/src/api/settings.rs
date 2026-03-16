//! User settings handlers — currency preference, display options, notifications.
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use crate::{AppError, AppResult, AppState, api::middleware::AuthUser, models::ApiResponse};

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct UserSettings {
    pub currency:              String,    // "USD" | "EUR" | "GBP" etc.
    pub language:              String,    // "en" | "de" | "fr" etc.
    pub show_nsfw:             bool,
    pub email_notifications:   bool,
    pub push_notifications:    bool,
    pub auto_play_media:       bool,
    pub compact_mode:          bool,
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
}

const SUPPORTED_CURRENCIES: &[&str] = &["USD", "EUR", "GBP", "CHF", "JPY", "BTC", "ETH", "BNB"];

pub async fn get_settings(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<UserSettings>>> {
    let settings = sqlx::query_as::<_, UserSettings>(
        "SELECT currency, language, show_nsfw, email_notifications,
                push_notifications, auto_play_media, compact_mode
         FROM user_settings WHERE user_id = $1"
    )
    .bind(auth.user_id)
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

    // Upsert settings
    sqlx::query(
        "INSERT INTO user_settings
            (user_id, currency, language, show_nsfw,
             email_notifications, push_notifications, auto_play_media, compact_mode)
         VALUES ($1,
             COALESCE($2, 'USD'), COALESCE($3, 'en'), COALESCE($4, false),
             COALESCE($5, true),  COALESCE($6, true), COALESCE($7, true),  COALESCE($8, false))
         ON CONFLICT (user_id) DO UPDATE SET
             currency            = COALESCE($2,  user_settings.currency),
             language            = COALESCE($3,  user_settings.language),
             show_nsfw           = COALESCE($4,  user_settings.show_nsfw),
             email_notifications = COALESCE($5,  user_settings.email_notifications),
             push_notifications  = COALESCE($6,  user_settings.push_notifications),
             auto_play_media     = COALESCE($7,  user_settings.auto_play_media),
             compact_mode        = COALESCE($8,  user_settings.compact_mode),
             updated_at          = NOW()"
    )
    .bind(auth.user_id)
    .bind(&req.currency)
    .bind(&req.language)
    .bind(req.show_nsfw)
    .bind(req.email_notifications)
    .bind(req.push_notifications)
    .bind(req.auto_play_media)
    .bind(req.compact_mode)
    .execute(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    get_settings(State(state), auth).await
}