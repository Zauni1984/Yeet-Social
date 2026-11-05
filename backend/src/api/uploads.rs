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

/// Audit I-M5: the declared type (Content-Type / file name) must match the
/// file's own signature, so a renamed HTML/SVG/script never lands under
/// /uploads with an image or media extension.
fn sniff_matches(ext: &str, b: &[u8]) -> bool {
    let at = |i: usize, s: &[u8]| b.len() >= i + s.len() && &b[i..i + s.len()] == s;
    match ext {
        "jpg"  => at(0, &[0xFF, 0xD8, 0xFF]),
        "png"  => at(0, &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
        "gif"  => at(0, b"GIF87a") || at(0, b"GIF89a"),
        "webp" => at(0, b"RIFF") && at(8, b"WEBP"),
        "mp4" | "mov" | "m4a" => at(4, b"ftyp"),
        "webm" | "weba"       => at(0, &[0x1A, 0x45, 0xDF, 0xA3]),
        "ogg"  => at(0, b"OggS"),
        "wav"  => at(0, b"RIFF") && at(8, b"WAVE"),
        "mp3"  => at(0, b"ID3") || (b.len() >= 2 && b[0] == 0xFF && (b[1] & 0xE0) == 0xE0),
        _ => false,
    }
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
    if !sniff_matches(ext, &bytes) {
        return Err(AppError::Validation("File content does not match its image type".into()));
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
/// Audio stories: up to 60 minutes. The recorder asks for 64 kbit/s
/// Opus/AAC (~29 MB per hour); 80 MB also covers a 60-minute MP3 at
/// 160 kbit/s picked from the file dialog. Route body limit: 96 MB.
const POST_AUDIO_MAX_BYTES: usize = 80 * 1024 * 1024; // 80 MB

#[derive(Clone, Copy, PartialEq, Eq)]
enum MediaKind { Image, Video, Audio }

fn post_media_ext(ct: Option<&str>, filename: Option<&str>) -> Option<(&'static str, MediaKind)> {
    use MediaKind::*;
    // MediaRecorder sends parameters ("audio/webm;codecs=opus") — match on the bare type.
    let ct = ct.unwrap_or("").split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    let name = filename.unwrap_or("").to_ascii_lowercase();
    match ct.as_str() {
        // Audio uses extensions distinct from video so the client can tell
        // an audio story from a clip by URL alone (.weba vs .webm).
        "audio/webm"                        => return Some(("weba", Audio)),
        "audio/ogg" | "audio/opus"          => return Some(("ogg", Audio)),
        "audio/mp4" | "audio/x-m4a" | "audio/m4a" | "audio/aac" => return Some(("m4a", Audio)),
        "audio/mpeg" | "audio/mp3"          => return Some(("mp3", Audio)),
        "audio/wav" | "audio/x-wav" | "audio/wave" => return Some(("wav", Audio)),
        "image/jpeg" | "image/jpg" => return Some(("jpg", Image)),
        "image/png"                => return Some(("png", Image)),
        "image/webp"               => return Some(("webp", Image)),
        "image/gif"                => return Some(("gif", Image)),
        "video/mp4" | "video/quicktime" => return Some(("mp4", Video)),
        "video/webm"               => return Some(("webm", Video)),
        _ => {}
    }
    if name.ends_with(".jpg") || name.ends_with(".jpeg") { return Some(("jpg", Image)); }
    if name.ends_with(".png")  { return Some(("png", Image)); }
    if name.ends_with(".webp") { return Some(("webp", Image)); }
    if name.ends_with(".gif")  { return Some(("gif", Image)); }
    if name.ends_with(".mp4")  { return Some(("mp4", Video)); }
    if name.ends_with(".mov")  { return Some(("mp4", Video)); } // we accept .mov as mp4 for now
    if name.ends_with(".webm") { return Some(("webm", Video)); }
    if name.ends_with(".weba") { return Some(("weba", Audio)); }
    if name.ends_with(".ogg") || name.ends_with(".oga") || name.ends_with(".opus") { return Some(("ogg", Audio)); }
    if name.ends_with(".m4a") || name.ends_with(".aac") { return Some(("m4a", Audio)); }
    if name.ends_with(".mp3")  { return Some(("mp3", Audio)); }
    if name.ends_with(".wav")  { return Some(("wav", Audio)); }
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
    let (ext, kind) = post_media_ext(ct.as_deref(), name.as_deref())
        .ok_or_else(|| AppError::Validation(
            "Allowed formats: JPG, PNG, WebP, GIF, MP4, MOV, WebM, and audio (WebM/Opus, M4A, MP3, OGG, WAV)".into()))?;

    let bytes = field.bytes().await
        .map_err(|e| AppError::Validation(format!("Read error: {e}")))?;
    if bytes.is_empty() {
        return Err(AppError::Validation("Empty file".into()));
    }
    // Per-kind size limit. Images keep the existing 8 MB cap; videos
    // get a separate 32 MB cap (~30 s at 720p H.264).
    let (cap, label) = match kind {
        MediaKind::Video => (POST_VIDEO_MAX_BYTES, "Video"),
        MediaKind::Audio => (POST_AUDIO_MAX_BYTES, "Audio"),
        MediaKind::Image => (MAX_BYTES, "Image"),
    };
    if bytes.len() > cap {
        return Err(AppError::Validation(format!(
            "{label} too large (max {} MB)", cap / (1024 * 1024)
        )));
    }
    if !sniff_matches(ext, &bytes) {
        return Err(AppError::Validation(format!("{label} content does not match its declared type")));
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

#[cfg(test)]
mod sniff_tests {
    use super::sniff_matches;
    #[test]
    fn signatures() {
        assert!(sniff_matches("png", &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0]));
        assert!(sniff_matches("jpg", &[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(sniff_matches("webp", b"RIFF\x00\x00\x00\x00WEBPVP8 "));
        assert!(sniff_matches("mp4", b"\x00\x00\x00\x18ftypisom"));
        assert!(sniff_matches("weba", &[0x1A, 0x45, 0xDF, 0xA3, 0x01]));
        assert!(sniff_matches("ogg", b"OggS\x00\x02"));
        assert!(sniff_matches("mp3", b"ID3\x04"));
        assert!(sniff_matches("mp3", &[0xFF, 0xFB, 0x90]));
        assert!(sniff_matches("wav", b"RIFF\x24\x08\x00\x00WAVEfmt "));
        // A renamed HTML/SVG/script never passes.
        assert!(!sniff_matches("png", b"<html><script>"));
        assert!(!sniff_matches("jpg", b"<svg xmlns"));
        assert!(!sniff_matches("mp4", b"GIF89a"));
        assert!(!sniff_matches("gif", b""));
    }
}
