//! Post translation endpoints.
//!
//! GET  /api/v1/translate/status      → is a provider configured? which languages?
//! POST /api/v1/posts/:id/translate   → {target} → translated text (cached per post+lang)
//!
//! Auth required for translating (rate-limited per principal so a hostile
//! client cannot burn the provider quota); status is public so the UI can
//! hide the button when nothing is configured.
use axum::{extract::{Path, State}, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::{
    api::middleware::AuthUser,
    models::ApiResponse,
    services::{rate_limit::{self, RateLimitOutcome}, translate},
    AppError, AppResult, AppState,
};

#[derive(Serialize)]
pub struct StatusResponse {
    pub enabled: bool,
    pub provider: Option<&'static str>,
    pub languages: Vec<&'static str>,
}

pub async fn status() -> Json<ApiResponse<StatusResponse>> {
    let cfg = translate::config();
    Json(ApiResponse::ok(StatusResponse {
        enabled: cfg.enabled(),
        provider: cfg.provider_name(),
        languages: translate::SUPPORTED.to_vec(),
    }))
}

#[derive(Deserialize)]
pub struct TranslateRequest { pub target: String }

#[derive(Serialize)]
pub struct TranslateResponse {
    pub text: String,
    pub source_lang: Option<String>,
    pub target_lang: String,
    pub cached: bool,
    /// True when the post already is in the target language: `text` is
    /// then the original and nothing was sent to the provider.
    pub same_language: bool,
}

pub async fn translate_post(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<TranslateRequest>,
) -> AppResult<Json<ApiResponse<TranslateResponse>>> {
    let cfg = translate::config();
    if !cfg.enabled() { return Err(AppError::Forbidden("TRANSLATION_DISABLED".into())); }
    let target = translate::normalize_lang(&req.target)
        .filter(|l| translate::is_supported(l))
        .ok_or_else(|| AppError::Validation("UNSUPPORTED_TARGET".into()))?;

    // 20 per minute burst, 300 per hour sustained — generous for a human
    // reading a feed, tight enough to keep one account from draining the
    // provider quota. Cached hits below are counted too (cheap anyway).
    match rate_limit::check_two_window(&state.cache, "translate", &auth.address, 60, 20, 3600, 300).await {
        RateLimitOutcome::Allowed => {}
        _ => return Err(AppError::RateLimited),
    }

    let row: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT content, lang FROM posts WHERE id = $1 AND deleted_at IS NULL AND is_removed = FALSE"
    ).bind(id).fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (content, stored_lang) = row.ok_or_else(|| AppError::NotFound("Post not found".into()))?;
    let known = translate::known_lang(stored_lang.as_deref());

    if known.as_deref() == Some(target.as_str()) {
        return Ok(Json(ApiResponse::ok(TranslateResponse {
            text: content, source_lang: known, target_lang: target, cached: true, same_language: true,
        })));
    }

    if let Some((text, source_lang)) = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT text, source_lang FROM post_translations WHERE post_id = $1 AND target_lang = $2"
    ).bind(id).bind(&target).fetch_optional(state.db.pool()).await.map_err(AppError::Database)? {
        return Ok(Json(ApiResponse::ok(TranslateResponse {
            text, source_lang, target_lang: target, cached: true, same_language: false,
        })));
    }

    let tr = translate::translate(&cfg, &content, &target, known.as_deref()).await.map_err(|e| {
        tracing::warn!("translate: post {id} → {target}: {e}");
        AppError::Internal("TRANSLATION_FAILED".into())
    })?;
    let source = tr.source_lang.clone().or(known.clone());

    // Remember the detected source on the post (only if we did not know it).
    if let (Some(src), None) = (&source, &known) {
        let _ = sqlx::query("UPDATE posts SET lang = $2 WHERE id = $1 AND (lang IS NULL OR lang = 'und')")
            .bind(id).bind(src).execute(state.db.pool()).await;
    }
    if source.as_deref() == Some(target.as_str()) {
        return Ok(Json(ApiResponse::ok(TranslateResponse {
            text: content, source_lang: source, target_lang: target, cached: false, same_language: true,
        })));
    }

    sqlx::query(
        "INSERT INTO post_translations (post_id, target_lang, source_lang, text, provider)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (post_id, target_lang) DO UPDATE
           SET text = EXCLUDED.text, source_lang = EXCLUDED.source_lang, provider = EXCLUDED.provider"
    )
    .bind(id).bind(&target).bind(&source).bind(&tr.text).bind(cfg.provider_name().unwrap_or("unknown"))
    .execute(state.db.pool()).await.map_err(AppError::Database)?;

    Ok(Json(ApiResponse::ok(TranslateResponse {
        text: tr.text, source_lang: source, target_lang: target, cached: false, same_language: false,
    })))
}
