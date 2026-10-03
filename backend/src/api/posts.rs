#![allow(dead_code)]
//! Post CRUD, likes, reshares, comments.
use axum::{extract::{Path, State}, Json};
use chrono::{Duration as ChronoDuration, Utc, DateTime};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, models::{ApiResponse, Comment, FeedPost}};
use crate::api::middleware::{AuthUser, OptionalAuth};
use crate::services::tokens::{self, rewards, RewardAction};

#[derive(Debug, Deserialize)]
pub struct CreatePostRequest {
    pub content: String,
    pub media_url: Option<String>,
    pub is_adult: Option<bool>,
    pub is_nft: Option<bool>,
    pub nft_price_yeet: Option<f64>,
    pub is_permanent: Option<bool>,
    pub ppv_price_yeet: Option<f64>,
    /// `text` (default) or `audio` (Audio Story: media_url required,
    /// caption optional).
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddCommentRequest { pub content: String }

/// Server-side guard for user-supplied media/avatar URLs. They are rendered
/// into `src="…"` attributes by the SPA, so anything but a plain same-origin
/// upload path or an https URL without quote/space/angle characters is
/// refused (stored-XSS vector found in the 2026-10 audit).
pub(crate) fn validate_media_url(url: &str) -> AppResult<()> {
    let u = url.trim();
    if u.len() > 512 {
        return Err(AppError::Validation("media_url too long".into()));
    }
    if !(u.starts_with("/uploads/") || u.starts_with("https://")) {
        return Err(AppError::Validation("media_url must be an /uploads/ path or an https URL".into()));
    }
    if u.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '"' | '\'' | '<' | '>' | '\\' | '`')) {
        return Err(AppError::Validation("media_url contains invalid characters".into()));
    }
    Ok(())
}

/// Posting suspension check shared by create_post and scheduled posts.
pub(crate) async fn ensure_not_posting_banned(pool: &sqlx::PgPool, user_id: Uuid) -> AppResult<()> {
    let ban: Option<(Option<chrono::DateTime<Utc>>, Option<String>)> = sqlx::query_as(
        "SELECT posting_banned_until, post_ban_reason FROM users WHERE id = $1"
    )
    .bind(user_id)
    .fetch_optional(pool).await.map_err(AppError::Database)?;
    if let Some((Some(until), reason)) = ban {
        if until > Utc::now() {
            let hours_left = (until - Utc::now()).num_hours();
            let msg = match reason {
                Some(r) if !r.is_empty() =>
                    format!("Posting is suspended for ~{} more hour(s): {}", hours_left, r),
                _ => format!("Posting is suspended for ~{} more hour(s).", hours_left),
            };
            return Err(AppError::Forbidden(msg));
        }
    }
    Ok(())
}

/// Visibility rule shared by every single-post read path: public, or the
/// viewer is the author, or the viewer follows the author. `$v` is the
/// viewer uuid parameter (nil uuid for anonymous callers).
pub(crate) fn visibility_sql(viewer_param: &str) -> String {
    format!("(COALESCE(p.visibility::text, 'public') = 'public' OR p.author_id = {v}::uuid \
             OR EXISTS (SELECT 1 FROM follows f WHERE f.follower_id = {v}::uuid AND f.following_id = p.author_id))", v = viewer_param)
}

pub async fn create_post(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreatePostRequest>,
) -> AppResult<Json<ApiResponse<Uuid>>> {
    if let Some(m) = req.media_url.as_deref() { validate_media_url(m)?; }
    for f in [req.ppv_price_yeet, req.nft_price_yeet].into_iter().flatten() {
        if !f.is_finite() || f < 0.0 || f > 1e9 {
            return Err(AppError::Validation("price must be a finite non-negative number".into()));
        }
    }
    let kind = match req.kind.as_deref().map(|k| k.trim()) {
        None | Some("") | Some("text") => "text",
        Some("audio") => "audio",
        Some(_) => return Err(AppError::Validation("kind must be text or audio".into())),
    };
    if kind == "audio" && req.media_url.as_deref().map(|m| m.trim().is_empty()).unwrap_or(true) {
        return Err(AppError::Validation("Audio story needs a media_url".into()));
    }
    if (kind != "audio" && req.content.trim().is_empty()) || req.content.trim().chars().count() > 420 {
        return Err(AppError::Validation("Post content must be 1-420 chars".into()));
    }
    // Support both wallet users (auth.address = "0x...") and email users (auth.address = "email:UUID")
    let user_id: Uuid = if let Some(uuid_str) = auth.address.strip_prefix("email:") {
        uuid_str.parse::<Uuid>().map_err(|_| AppError::NotFound("Invalid user ID".into()))?
    } else {
        sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
            .bind(&auth.address)
            .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
            .ok_or_else(|| AppError::NotFound("User not found".into()))?
    };

    // Refuse if the user has an active posting ban (set via the
    // admin moderation API). The reason, if any, is surfaced so the
    // client can show a meaningful message rather than a generic 403.
    let ban: Option<(Option<chrono::DateTime<Utc>>, Option<String>)> = sqlx::query_as(
        "SELECT posting_banned_until, post_ban_reason FROM users WHERE id = $1"
    )
    .bind(user_id)
    .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    if let Some((Some(until), reason)) = ban {
        if until > Utc::now() {
            let hours_left = (until - Utc::now()).num_hours();
            let msg = match reason {
                Some(r) if !r.is_empty() =>
                    format!("Posting is suspended for ~{} more hour(s): {}", hours_left, r),
                _ => format!("Posting is suspended for ~{} more hour(s).", hours_left),
            };
            return Err(AppError::Forbidden(msg));
        }
    }

    let is_permanent = req.is_permanent.unwrap_or(false) || req.is_nft.unwrap_or(false);
    let expires_at = if is_permanent {
        Utc::now() + ChronoDuration::hours(24 * 365 * 100)
    } else {
        Utc::now() + ChronoDuration::hours(24)
    };
    let media_url_clone = req.media_url.clone();
    let media_arr: Vec<String> = req.media_url.into_iter().collect();
    let post_id: Uuid = sqlx::query_scalar(
        "INSERT INTO posts (author_id, content, media_urls, media_url, expires_at, is_adult, is_nft, nft_price_yeet, is_permanent, ppv_price_yeet, kind)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) RETURNING id"
    )
    .bind(user_id).bind(&req.content).bind(&media_arr)
    .bind(media_url_clone.as_deref())
    .bind(expires_at)
    .bind(req.is_adult.unwrap_or(false))
    .bind(req.is_nft.unwrap_or(false))
    .bind(req.nft_price_yeet)
    // Store the *computed* permanence (is_permanent OR is_nft). Previously
    // this bound the raw request flag while expires_at was derived from the
    // OR — so an NFT post got a 100-year expiry (forever visible in the
    // global feed) yet is_permanent=FALSE, making it invisible to the
    // per-user permanent list. Bind the same value used for expires_at.
    .bind(is_permanent)
    .bind(req.ppv_price_yeet)
    .bind(kind)
    .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;

    // Posting reward: only articles of at least the configured length earn
    // points (spec: >=120 chars). Shorter posts still publish, just unrewarded.
    // The per-user daily cap is enforced inside grant_reward.
    if req.content.trim().chars().count() >= tokens::post_min_chars() {
        let _ = tokens::grant_reward(&state.db, user_id, RewardAction::PostCreated, tokens::post_reward()).await;
    }
    // Tag the post language right away (the 30 s sweep would catch it too,
    // but the Translate button should be right on first render).
    crate::services::translate::spawn_detect(state.clone(), post_id, req.content.clone());
    Ok(Json(ApiResponse::ok(post_id)))
}

/// GET /api/v1/posts/:id — one post, same shape as a feed card. Anonymous
/// callers are allowed (public links); PPV unlock state is resolved for the
/// signed-in viewer when there is one.
pub async fn get_post(
    State(state): State<AppState>,
    OptionalAuth(auth): OptionalAuth,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<FeedPost>>> {
    let viewer_id = match auth.as_ref() {
        Some(a) => crate::api::feed::resolve_viewer_id(&state, a).await.ok(),
        None => None,
    };
    let post = crate::api::feed::fetch_post(&state, id, viewer_id).await?
        .ok_or_else(|| AppError::NotFound("Post not found".into()))?;
    Ok(Json(ApiResponse::ok(post)))
}

pub async fn delete_post(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<()>>> {
    // Support both wallet users (auth.address = "0x...") and email users (auth.address = "email:UUID")
    let user_id: Uuid = if let Some(uuid_str) = auth.address.strip_prefix("email:") {
        uuid_str.parse::<Uuid>().map_err(|_| AppError::NotFound("Invalid user ID".into()))?
    } else {
        sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
            .bind(&auth.address)
            .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
            .ok_or_else(|| AppError::NotFound("User not found".into()))?
    };

    let result = sqlx::query(
        "UPDATE posts SET deleted_at = NOW() WHERE id = $1 AND author_id = $2 AND is_nft = false AND deleted_at IS NULL"
    )
    .bind(id).bind(user_id)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Post not found or cannot be deleted".into()));
    }
    Ok(Json(ApiResponse::ok(())))
}

pub async fn like_post(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<()>>> {
    // Support both wallet users (auth.address = "0x...") and email users (auth.address = "email:UUID")
    let user_id: Uuid = if let Some(uuid_str) = auth.address.strip_prefix("email:") {
        uuid_str.parse::<Uuid>().map_err(|_| AppError::NotFound("Invalid user ID".into()))?
    } else {
        sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
            .bind(&auth.address)
            .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
            .ok_or_else(|| AppError::NotFound("User not found".into()))?
    };

    let inserted = sqlx::query(
        "INSERT INTO post_likes (post_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING"
    )
    .bind(id).bind(user_id)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;

    if inserted.rows_affected() > 0 {
        sqlx::query("UPDATE posts SET like_count = like_count + 1 WHERE id = $1")
            .bind(id).execute(state.db.pool()).await.map_err(AppError::Database)?;
        // Notify the post author (notify() skips self-notifications).
        if let Some(author_id) = sqlx::query_scalar::<_, Uuid>(
            "SELECT author_id FROM posts WHERE id = $1"
        ).bind(id).fetch_optional(state.db.pool()).await.ok().flatten() {
            let actor = sqlx::query_scalar::<_, Option<String>>(
                "SELECT COALESCE(display_name, username) FROM users WHERE id = $1"
            ).bind(user_id).fetch_optional(state.db.pool()).await
             .ok().flatten().flatten().unwrap_or_else(|| "Someone".into());
            crate::api::notifications::notify(
                state.db.pool(), author_id, Some(user_id),
                "like", &format!("{} liked your post", actor), Some(id),
            ).await;
        }
    }
    Ok(Json(ApiResponse::ok(())))
}

pub async fn unlike_post(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<Uuid>>> {
    let user_id: Uuid = if let Some(uuid_str) = auth.address.strip_prefix("email:") {
        uuid_str.parse::<Uuid>().map_err(|_| AppError::NotFound("Invalid user ID".into()))?
    } else {
        // Wallet users must be resolved via the DB; parsing a 0x address as a
        // UUID always failed and fell back to Uuid::nil(), so the DELETE
        // matched nothing and wallet users could never unlike (200 OK, no-op).
        sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
            .bind(&auth.address)
            .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
            .ok_or_else(|| AppError::NotFound("User not found".into()))?
    };

    let removed = sqlx::query(
        "DELETE FROM post_likes WHERE post_id = $1 AND user_id = $2"
    )
    .bind(id)
    .bind(user_id)
    .execute(state.db.pool())
    .await
    .map_err(AppError::Database)?;

    // Only decrement when this user actually had a like — otherwise a loop of
    // unlikes could zero any post's count (audit C-M2).
    if removed.rows_affected() > 0 {
        let _ = sqlx::query("UPDATE posts SET like_count = GREATEST(0, like_count - 1) WHERE id = $1")
            .bind(id)
            .execute(state.db.pool())
            .await;
    }

    Ok(Json(ApiResponse::ok(id)))
}


#[derive(Debug, Serialize)]
pub struct UnlockResponse {
    pub unlocked: bool,
    pub price_paid: f64,
    pub tip_id: Option<Uuid>,
    pub already_unlocked: bool,
}

/// Version of the pay-per-view consent notice the client must show before
/// an unlock (F7). Bump when the wording changes; the accepted version is
/// stored on the unlock row.
pub const PPV_CONSENT_VERSION: &str = "2026-10-01";

/// Optional JSON body of POST /posts/:id/unlock. Older clients send no body
/// at all, so this is parsed by hand rather than via the Json extractor.
#[derive(Debug, Default, Deserialize)]
pub struct UnlockRequest {
    #[serde(default)]
    pub consent: bool,
    #[serde(default)]
    pub consent_version: Option<String>,
    #[serde(default)]
    pub lang: Option<String>,
}

pub fn parse_unlock_body(body: &[u8]) -> Result<UnlockRequest, AppError> {
    if body.iter().all(|b| b.is_ascii_whitespace()) { return Ok(UnlockRequest::default()); }
    serde_json::from_slice(body).map_err(|_| AppError::Validation("Malformed JSON body".into()))
}

/// One row of GET /api/v1/me/ppv-unlocks — the buyer's durable record of a
/// pay-per-view purchase incl. the consent they gave (F7).
#[derive(Debug, Serialize)]
pub struct PpvUnlockReceipt {
    pub post_id: Uuid,
    pub author_name: Option<String>,
    pub author_username: Option<String>,
    pub content_preview: Option<String>,
    pub price_paid: f64,
    pub unlocked_at: DateTime<Utc>,
    pub consent_version: Option<String>,
    pub consent_at: Option<DateTime<Utc>>,
    pub consent_lang: Option<String>,
}

pub async fn list_my_ppv_unlocks(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<Vec<PpvUnlockReceipt>>>> {
    let uid = crate::api::conversations::caller_user_id(&state, &auth).await?;
    let rows: Vec<(Uuid, Option<String>, Option<String>, Option<String>, f64, DateTime<Utc>, Option<String>, Option<DateTime<Utc>>, Option<String>)> = sqlx::query_as(
        "SELECT pu.post_id, u.display_name, u.username, LEFT(p.content, 80),
                CAST(pu.price_paid AS DOUBLE PRECISION), pu.unlocked_at,
                pu.consent_version, pu.consent_at, pu.consent_lang
           FROM ppv_unlocks pu
           JOIN posts p ON p.id = pu.post_id
           JOIN users u ON u.id = p.author_id
          WHERE pu.user_id = $1
          ORDER BY pu.unlocked_at DESC
          LIMIT 200"
    )
    .bind(uid)
    .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;
    let out = rows.into_iter().map(|r| PpvUnlockReceipt {
        post_id: r.0, author_name: r.1, author_username: r.2, content_preview: r.3,
        price_paid: r.4, unlocked_at: r.5, consent_version: r.6, consent_at: r.7, consent_lang: r.8,
    }).collect();
    Ok(Json(ApiResponse::ok(out)))
}

/// Pay-per-view unlock. The post author sets `ppv_price_yeet`; another
/// authenticated viewer calls this endpoint to debit the price from
/// their balance (10% platform cut, 90% to the author via the standard
/// tips ledger), and a `ppv_unlocks` row records the entitlement so the
/// viewer never gets re-charged. Idempotent.
///
/// F7: a NEW purchase requires the buyer's express consent to immediate
/// performance (and acknowledgement of the lost right of withdrawal) for
/// the current `PPV_CONSENT_VERSION`; the consent is stored on the unlock
/// row and confirmed by email on a durable medium where an address exists.
pub async fn unlock_post(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    body: axum::body::Bytes,
) -> AppResult<Json<ApiResponse<UnlockResponse>>> {
    let caller_id = crate::api::conversations::caller_user_id(&state, &auth).await?;
    let req = parse_unlock_body(&body)?;

    // Resolve post + price + author in one shot.
    let row: Option<(Uuid, Option<f64>, bool, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT author_id,
                CAST(ppv_price_yeet AS DOUBLE PRECISION) AS price,
                is_removed,
                deleted_at
           FROM posts WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(state.db.pool())
    .await
    .map_err(AppError::Database)?;
    let (author_id, price_opt, is_removed, deleted_at) =
        row.ok_or_else(|| AppError::NotFound("Post not found".into()))?;
    if is_removed || deleted_at.is_some() {
        return Err(AppError::NotFound("Post not found".into()));
    }
    let price = match price_opt {
        Some(p) if p > 0.0 => p,
        _ => return Err(AppError::Validation("Post is not pay-per-view".into())),
    };
    if author_id == caller_id {
        return Err(AppError::Validation("Cannot unlock your own post".into()));
    }
    if crate::api::blocks::either_blocks(state.db.pool(), caller_id, author_id).await? {
        return Err(AppError::Forbidden("Blocked".into()));
    }

    // Idempotent path: if an unlock row already exists, just return it.
    let existing: Option<(f64, Option<Uuid>)> = sqlx::query_as(
        "SELECT CAST(price_paid AS DOUBLE PRECISION), tip_id
           FROM ppv_unlocks WHERE user_id = $1 AND post_id = $2"
    )
    .bind(caller_id).bind(id)
    .fetch_optional(state.db.pool())
    .await
    .map_err(AppError::Database)?;
    if let Some((paid, tid)) = existing {
        return Ok(Json(ApiResponse::ok(UnlockResponse {
            unlocked: true,
            price_paid: paid,
            tip_id: tid,
            already_unlocked: true,
        })));
    }

    // F7 — no consent, no purchase. The client shows the notice and sends
    // consent=true with the version it displayed; an outdated version means
    // the user saw stale wording, so it is treated like no consent.
    if !req.consent || req.consent_version.as_deref() != Some(PPV_CONSENT_VERSION) {
        return Err(AppError::Validation("CONSENT_REQUIRED".into()));
    }
    let consent_lang = match req.lang.as_deref().map(str::trim) { Some("de") => "de", _ => "en" };
    let consent_at = Utc::now();

    // Atomic charge + record. Fee accounting reuses the tips path so the
    // creator's 90% / platform 10% split is consistent with other tips.
    let mut tx = state.db.pool().begin().await.map_err(AppError::Database)?;
    let tip_id = crate::api::tips::send_tip_tx(
        &mut tx, caller_id, author_id, Some(id),
        &price.to_string(), "YEET", None, crate::api::tips::TipKind::PayPerView,
    ).await?;
    sqlx::query(
        "INSERT INTO ppv_unlocks (user_id, post_id, price_paid, tip_id, consent_version, consent_at, consent_lang)
         VALUES ($1, $2, $3, $4, $5, $6, $7)"
    )
    .bind(caller_id).bind(id).bind(price).bind(tip_id)
    .bind(PPV_CONSENT_VERSION).bind(consent_at).bind(consent_lang)
    .execute(&mut *tx).await.map_err(AppError::Database)?;
    tx.commit().await.map_err(AppError::Database)?;

    // Durable-medium confirmation (§ 312f BGB): email where we have one.
    // Best-effort and off the request path; wallet-only users keep the
    // in-app record under Token Tips → Pay-per-View purchases.
    let email: Option<String> = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(caller_id).fetch_optional(state.db.pool()).await.ok().flatten();
    if let (Some(addr), Some(cfg)) = (email, crate::services::email::EmailConfig::from_env()) {
        let lang = consent_lang.to_string();
        tokio::spawn(async move {
            if let Err(e) = crate::services::email::send_ppv_confirmation(&cfg, &addr, &lang, id, price, consent_at).await {
                tracing::warn!("ppv: confirmation email to {addr} failed: {e}");
            }
        });
    }

    Ok(Json(ApiResponse::ok(UnlockResponse {
        unlocked: true,
        price_paid: price,
        tip_id: Some(tip_id),
        already_unlocked: false,
    })))
}

pub async fn reshare_post(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<()>>> {
    // Support both wallet users (auth.address = "0x...") and email users (auth.address = "email:UUID")
    let user_id: Uuid = if let Some(uuid_str) = auth.address.strip_prefix("email:") {
        uuid_str.parse::<Uuid>().map_err(|_| AppError::NotFound("Invalid user ID".into()))?
    } else {
        sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
            .bind(&auth.address)
            .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
            .ok_or_else(|| AppError::NotFound("User not found".into()))?
    };

    // One reshare per user and post, only for live posts that are not the
    // caller's own (audit C-M1: unbounded reshares could keep any post alive
    // forever and farm rewards).
    let inserted = sqlx::query(
        "INSERT INTO post_reshares (post_id, user_id)
         SELECT p.id, $2 FROM posts p
          WHERE p.id = $1 AND p.deleted_at IS NULL AND p.is_removed = FALSE
            AND p.expires_at > NOW() AND p.author_id <> $2
         ON CONFLICT DO NOTHING"
    )
    .bind(id).bind(user_id)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;
    if inserted.rows_affected() == 0 {
        return Err(AppError::Conflict("Already reshared, or post not available".into()));
    }
    let new_expiry = Utc::now() + ChronoDuration::hours(24);
    sqlx::query(
        "UPDATE posts SET reshare_count = reshare_count + 1, expires_at = GREATEST(expires_at, $1) WHERE id = $2"
    )
    .bind(new_expiry).bind(id)
    .execute(state.db.pool()).await.map_err(AppError::Database)?;

    let _ = tokens::grant_reward(&state.db, user_id, RewardAction::PostReshared, rewards::POST_RESHARED).await;
    Ok(Json(ApiResponse::ok(())))
}

pub async fn get_comments(
    State(state): State<AppState>,
    OptionalAuth(auth): OptionalAuth,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<Vec<Comment>>>> {
    // Comments follow the post: removed/deleted posts have no readable
    // comments, followers-only and adult posts only for eligible viewers.
    let viewer = match auth.as_ref() {
        Some(a) => crate::api::feed::resolve_viewer_id(&state, a).await.ok(),
        None => None,
    }.unwrap_or(Uuid::nil());
    let sql = format!(
        "SELECT c.id, c.post_id, c.author_id, c.content, c.created_at
           FROM comments c JOIN posts p ON p.id = c.post_id
          WHERE c.post_id = $1 AND p.deleted_at IS NULL AND p.is_removed = FALSE
            AND {vis}
            AND (p.is_adult = FALSE OR p.author_id = $2::uuid
                 OR EXISTS (SELECT 1 FROM users v WHERE v.id = $2::uuid AND v.age_verified_at IS NOT NULL))
          ORDER BY c.created_at ASC",
        vis = visibility_sql("$2"));
    let comments = sqlx::query_as::<_, Comment>(&sql)
        .bind(id).bind(viewer)
        .fetch_all(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(comments)))
}

pub async fn add_comment(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<AddCommentRequest>,
) -> AppResult<Json<ApiResponse<Uuid>>> {
    if req.content.trim().is_empty() || req.content.trim().chars().count() > 280 {
        return Err(AppError::Validation("Comment must be 1-280 chars".into()));
    }
    // Support both wallet users (auth.address = "0x...") and email users (auth.address = "email:UUID")
    let user_id: Uuid = if let Some(uuid_str) = auth.address.strip_prefix("email:") {
        uuid_str.parse::<Uuid>().map_err(|_| AppError::NotFound("Invalid user ID".into()))?
    } else {
        sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
            .bind(&auth.address)
            .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
            .ok_or_else(|| AppError::NotFound("User not found".into()))?
    };

    let target: Option<(Uuid,)> = sqlx::query_as(&format!(
        "SELECT p.author_id FROM posts p
          WHERE p.id = $1 AND p.deleted_at IS NULL AND p.is_removed = FALSE AND p.expires_at > NOW()
            AND {vis}", vis = visibility_sql("$2")))
        .bind(id).bind(user_id)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (post_author,) = target.ok_or_else(|| AppError::NotFound("Post not found".into()))?;
    if post_author != user_id && crate::api::blocks::either_blocks(state.db.pool(), user_id, post_author).await? {
        return Err(AppError::Forbidden("You cannot comment on this post".into()));
    }
    let comment_id: Uuid = sqlx::query_scalar(
        "INSERT INTO comments (post_id, author_id, content) VALUES ($1, $2, $3) RETURNING id"
    )
    .bind(id).bind(user_id).bind(&req.content)
    .fetch_one(state.db.pool()).await.map_err(AppError::Database)?;

    sqlx::query("UPDATE posts SET comment_count = comment_count + 1 WHERE id = $1")
        .bind(id).execute(state.db.pool()).await.map_err(AppError::Database)?;

    let _ = tokens::grant_reward(&state.db, user_id, RewardAction::CommentPosted, rewards::COMMENT_POSTED).await;

    // Notify the post author so they know someone replied.
    if let Some(author_id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT author_id FROM posts WHERE id = $1"
    ).bind(id).fetch_optional(state.db.pool()).await.ok().flatten() {
        let actor = sqlx::query_scalar::<_, Option<String>>(
            "SELECT COALESCE(display_name, username) FROM users WHERE id = $1"
        ).bind(user_id).fetch_optional(state.db.pool()).await
         .ok().flatten().flatten().unwrap_or_else(|| "Someone".into());
        crate::api::notifications::notify(
            state.db.pool(), author_id, Some(user_id),
            "comment", &format!("{} commented on your post", actor), Some(id),
        ).await;
    }

    Ok(Json(ApiResponse::ok(comment_id)))
}

pub async fn mint_nft(
    State(_state): State<AppState>,
    _auth: AuthUser,
    Path(_id): Path<Uuid>,
) -> AppResult<Json<ApiResponse<String>>> {
    Err(AppError::Internal("NFT minting not yet available".into()))
}

#[cfg(test)]
mod ppv_consent_tests {
    use super::*;

    #[test]
    fn empty_body_means_no_consent() {
        let r = parse_unlock_body(b"").unwrap();
        assert!(!r.consent && r.consent_version.is_none());
        let r = parse_unlock_body(b"  \n").unwrap();
        assert!(!r.consent);
    }

    #[test]
    fn json_body_is_parsed_and_junk_rejected() {
        let r = parse_unlock_body(br#"{"consent":true,"consent_version":"2026-10-01","lang":"de"}"#).unwrap();
        assert!(r.consent);
        assert_eq!(r.consent_version.as_deref(), Some(PPV_CONSENT_VERSION));
        assert_eq!(r.lang.as_deref(), Some("de"));
        assert!(parse_unlock_body(b"{not json").is_err());
    }
}
