//! Avatar + cover image upload handlers. Stores files on disk under
//! `UPLOADS_DIR` (default `/app/uploads`) and serves them via `ServeDir`
//! mounted at `/uploads` in main.rs.

use axum::{extract::{Multipart, State}, Json};
use rand::Rng;
use serde::Serialize;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;
use crate::{AppError, AppResult, AppState, api::middleware::AuthUser, models::ApiResponse};

const MAX_BYTES: usize = 8 * 1024 * 1024; // 8 MB

#[derive(Debug, Serialize)]
pub struct UploadResponse {
    pub url: String,
}

pub fn uploads_dir() -> PathBuf {
    PathBuf::from(std::env::var("UPLOADS_DIR").unwrap_or_else(|_| "/app/uploads".into()))
}

async fn resolve_user_id(state: &AppState, auth_address: &str) -> AppResult<Uuid> {
    if let Some(uuid_str) = auth_address.strip_prefix("email:") {
        return uuid_str.parse::<Uuid>().map_err(|_| AppError::NotFound("Invalid user ID".into()));
    }
    sqlx::query_scalar("SELECT id FROM users WHERE wallet_address = $1")
        .bind(auth_address)
        .fetch_optional(state.db.pool()).await.map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("User not found".into()))
}

/// Accept only common image types. Returns the file extension to use on disk.
fn ext_for(ct: Option<&str>, filename: Option<&str>) -> Option<&'static str> {
    let ct = ct.unwrap_or("").to_ascii_lowercase();
    let lower_name = filename.unwrap_or("").to_ascii_lowercase();
    match ct.as_str() {
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/png"                => Some("png"),
        "image/webp"               => Some("webp"),
        "image/gif"                => Some("gif"),
        _ if lower_name.ends_with(".jpg") || lower_name.ends_with(".jpeg") => Some("jpg"),
        _ if lower_name.ends_with(".png")  => Some("png"),
        _ if lower_name.ends_with(".webp") => Some("webp"),
        _ if lower_name.ends_with(".gif")  => Some("gif"),
        _ => None,
    }
}

/// Extract a single image file from a multipart body, writing it to
/// `<uploads_dir>/<subdir>/<user_id>-<rand>.<ext>`.
/// Returns the public URL (`/uploads/<subdir>/<file>`).
async fn save_image(
    state: &AppState,
    auth: &AuthUser,
    mut mp: Multipart,
    subdir: &str,
) -> AppResult<(Uuid, String)> {
    let user_id = resolve_user_id(state, &auth.address).await?;

    let field = mp.next_field().await
        .map_err(|e| AppError::Validation(format!("Multipart parse error: {e}")))?
        .ok_or_else(|| AppError::Validation("No file field".into()))?;

    let ct = field.content_type().map(|s| s.to_string());
    let name = field.file_name().map(|s| s.to_string());
    let ext = ext_for(ct.as_deref(), name.as_deref())
        .ok_or_else(|| AppError::Validation("Only JPG, PNG, WebP, GIF allowed".into()))?;

    let bytes = field.bytes().await
        .map_err(|e| AppError::Validation(format!("Read error: {e}")))?;
    if bytes.len() > MAX_BYTES {
        return Err(AppError::Validation("Image too large (max 8 MB)".into()));
    }
    if bytes.is_empty() {
        return Err(AppError::Validation("Empty file".into()));
    }

    let dir = uploads_dir().join(subdir);
    tokio::fs::create_dir_all(&dir).await
        .map_err(|e| AppError::Internal(format!("mkdir: {e}")))?;

    let rand_suffix: u32 = rand::thread_rng().gen();
    let filename = format!("{user_id}-{rand_suffix:08x}.{ext}");
    let path = dir.join(&filename);
    let mut f = tokio::fs::File::create(&path).await
        .map_err(|e| AppError::Internal(format!("create: {e}")))?;
    f.write_all(&bytes).await.map_err(|e| AppError::Internal(format!("write: {e}")))?;
    f.flush().await.map_err(|e| AppError::Internal(format!("flush: {e}")))?;

    let url = format!("/uploads/{subdir}/{filename}");
    Ok((user_id, url))
}

pub async fn upload_avatar(
    State(state): State<AppState>,
    auth: AuthUser,
    mp: Multipart,
) -> AppResult<Json<ApiResponse<UploadResponse>>> {
    let (user_id, url) = save_image(&state, &auth, mp, "avatars").await?;
    sqlx::query("UPDATE users SET avatar_url = $2, updated_at = NOW() WHERE id = $1")
        .bind(user_id).bind(&url)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(UploadResponse { url })))
}

pub async fn upload_cover(
    State(state): State<AppState>,
    auth: AuthUser,
    mp: Multipart,
) -> AppResult<Json<ApiResponse<UploadResponse>>> {
    let (user_id, url) = save_image(&state, &auth, mp, "covers").await?;
    sqlx::query("UPDATE users SET cover_url = $2, updated_at = NOW() WHERE id = $1")
        .bind(user_id).bind(&url)
        .execute(state.db.pool()).await.map_err(AppError::Database)?;
    Ok(Json(ApiResponse::ok(UploadResponse { url })))
}

/// Post media upload — images OR short videos.
///
/// Returns a public /uploads/posts-media/<file> URL. The frontend
/// stores it as posts.media_url. The orphan-cleanup sweep in the
/// background services purges any file whose URL no longer appears
/// in the posts table (matching the 24h ephemeral retention rule).
const POST_VIDEO_MAX_BYTES: usize = 32 * 1024 * 1024; // 32 MB

fn post_media_ext(ct: Option<&str>, filename: Option<&str>) -> Option<(&'static str, bool)> {
    // Returns (extension, is_video).
    let ct = ct.unwrap_or("").to_ascii_lowercase();
    let name = filename.unwrap_or("").to_ascii_lowercase();
    match ct.as_str() {
        "image/jpeg" | "image/jpg" => return Some(("jpg", false)),
        "image/png"                => return Some(("png", false)),
        "image/webp"               => return Some(("webp", false)),
        "image/gif"                => return Some(("gif", false)),
        "video/mp4" | "video/quicktime" => return Some(("mp4", true)),
        "video/webm"               => return Some(("webm", true)),
        _ => {}
    }
    if name.ends_with(".jpg") || name.ends_with(".jpeg") { return Some(("jpg", false)); }
    if name.ends_with(".png")  { return Some(("png", false)); }
    if name.ends_with(".webp") { return Some(("webp", false)); }
    if name.ends_with(".gif")  { return Some(("gif", false)); }
    if name.ends_with(".mp4")  { return Some(("mp4", true)); }
    if name.ends_with(".mov")  { return Some(("mp4", true)); } // we accept .mov as mp4 for now
    if name.ends_with(".webm") { return Some(("webm", true)); }
    None
}

pub async fn upload_post_media(
    State(state): State<AppState>,
    auth: AuthUser,
    mut mp: Multipart,
) -> AppResult<Json<ApiResponse<UploadResponse>>> {
    let user_id = resolve_user_id(&state, &auth.address).await?;

    let field = mp.next_field().await
        .map_err(|e| AppError::Validation(format!("Multipart parse error: {e}")))?
        .ok_or_else(|| AppError::Validation("No file field".into()))?;
    let ct = field.content_type().map(|s| s.to_string());
    let name = field.file_name().map(|s| s.to_string());
    let (ext, is_video) = post_media_ext(ct.as_deref(), name.as_deref())
        .ok_or_else(|| AppError::Validation(
            "Allowed formats: JPG, PNG, WebP, GIF, MP4, MOV, WebM".into()))?;

    let bytes = field.bytes().await
        .map_err(|e| AppError::Validation(format!("Read error: {e}")))?;
    if bytes.is_empty() {
        return Err(AppError::Validation("Empty file".into()));
    }
    // Per-kind size limit. Images keep the existing 8 MB cap; videos
    // get a separate 32 MB cap (~30 s at 720p H.264).
    let cap = if is_video { POST_VIDEO_MAX_BYTES } else { MAX_BYTES };
    if bytes.len() > cap {
        return Err(AppError::Validation(format!(
            "{} too large (max {} MB)",
            if is_video { "Video" } else { "Image" },
            cap / (1024 * 1024)
        )));
    }

    let dir = uploads_dir().join("posts-media");
    tokio::fs::create_dir_all(&dir).await
        .map_err(|e| AppError::Internal(format!("mkdir: {e}")))?;

    let rand_suffix: u32 = rand::thread_rng().gen();
    let filename = format!("{user_id}-{rand_suffix:08x}.{ext}");
    let path = dir.join(&filename);
    let mut f = tokio::fs::File::create(&path).await
        .map_err(|e| AppError::Internal(format!("create: {e}")))?;
    f.write_all(&bytes).await.map_err(|e| AppError::Internal(format!("write: {e}")))?;
    f.flush().await.map_err(|e| AppError::Internal(format!("flush: {e}")))?;

    let url = format!("/uploads/posts-media/{filename}");
    Ok(Json(ApiResponse::ok(UploadResponse { url })))
}
