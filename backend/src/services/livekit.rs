//! LiveKit token minting + config loading.
//!
//! Phase 2 ships the wiring; the actual LiveKit server runs separately
//! (self-hosted via the official Docker image — see README_LIVEKIT.md).
//!
//! Tokens are HS256 JWTs whose `video` claim grants room-scoped
//! publisher or subscriber rights. Server-side this is the only crypto
//! the client needs to talk WebRTC to LiveKit; the actual RTP/SRTP
//! happens between browser and LiveKit and never touches us.
//!
//! Failure mode if env is unset: `config_from_env()` returns None and
//! the calling endpoints reply 503 with a clear "LIVE_NOT_CONFIGURED"
//! error code. Front-end falls back to the Phase-1 placeholder.
use serde::{Deserialize, Serialize};
use jsonwebtoken::{encode, EncodingKey, Header, Algorithm};

#[derive(Debug, Clone)]
pub struct LiveKitConfig {
    /// e.g. `wss://livekit.justyeet.it`
    pub ws_url: String,
    pub api_key: String,
    pub api_secret: String,
}

pub fn config_from_env() -> Option<LiveKitConfig> {
    let ws_url = std::env::var("LIVEKIT_WS_URL").ok()?;
    let api_key = std::env::var("LIVEKIT_API_KEY").ok()?;
    let api_secret = std::env::var("LIVEKIT_API_SECRET").ok()?;
    if ws_url.is_empty() || api_key.is_empty() || api_secret.is_empty() {
        return None;
    }
    Some(LiveKitConfig { ws_url, api_key, api_secret })
}

#[derive(Debug, Serialize, Deserialize)]
struct VideoGrant {
    /// "room" scope name.
    room: String,
    #[serde(rename = "roomJoin")]
    room_join: bool,
    #[serde(rename = "canPublish")]
    can_publish: bool,
    #[serde(rename = "canSubscribe")]
    can_subscribe: bool,
    #[serde(rename = "canPublishData")]
    can_publish_data: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct LiveKitClaims {
    /// API key — LiveKit looks up the secret by `iss` to verify HS256.
    iss: String,
    /// Stable identity (we use the user UUID).
    sub: String,
    /// Display name shown in LiveKit Room.participants.
    name: String,
    nbf: i64,
    exp: i64,
    video: VideoGrant,
}

/// Mint a LiveKit access token. `ttl_seconds` controls how long the
/// token is valid for; 6h is a sensible default for live broadcasts.
pub fn mint_token(
    cfg: &LiveKitConfig,
    identity: &str,
    display_name: &str,
    room: &str,
    can_publish: bool,
    ttl_seconds: i64,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = chrono::Utc::now().timestamp();
    let claims = LiveKitClaims {
        iss: cfg.api_key.clone(),
        sub: identity.to_string(),
        name: display_name.to_string(),
        nbf: now - 5,           // small clock-skew tolerance
        exp: now + ttl_seconds,
        video: VideoGrant {
            room: room.to_string(),
            room_join: true,
            can_publish,
            can_subscribe: true,
            can_publish_data: true,
        },
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(cfg.api_secret.as_bytes()),
    )
}

pub fn room_name_for(live_id: uuid::Uuid) -> String {
    // Plain stable string — LiveKit accepts arbitrary room names.
    format!("yeet-{live_id}")
}
