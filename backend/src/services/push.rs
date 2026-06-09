//! Web Push (VAPID + tickle pushes).
//!
//! We only ship "tickle" pushes — zero payload, just a wake-up
//! signal. The service worker on the client renders a generic
//! "New message" notification and the user opens the app to see the
//! actual ciphertext. That protects two things:
//!
//!   * Push services (FCM, Mozilla autopush, Apple) never see
//!     plaintext content.
//!   * We avoid implementing the aes128gcm payload encryption from
//!     RFC 8291 in this commit. The schema (p256dh_key + auth_key)
//!     is stored so a later upgrade to encrypted payloads is just a
//!     send-side change.
//!
//! Auth uses VAPID (RFC 8292): the request includes a JWT signed
//! with ES256 (ECDSA P-256 + SHA-256) over the push service origin.
//! The receiving push service validates the JWT against our static
//! VAPID public key, which the browser bound to the subscription at
//! subscribe time.
//!
//! Failure mode if VAPID env vars are unset: `config_from_env()`
//! returns None and the callers no-op. The REST + WS paths keep
//! working; users just don't get pushes until the operator deploys
//! the keys.

use base64::Engine;
use p256::ecdsa::{signature::Signer, Signature, SigningKey};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const URL_SAFE_NO_PAD: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// Operator-controlled VAPID identity. Generated once with
/// `openssl ecparam -name prime256v1 -genkey -noout` (see
/// README_LIVEKIT.md-style ops doc) and pinned via env so restarts
/// don't invalidate existing subscriptions.
#[derive(Debug, Clone)]
pub struct VapidConfig {
    /// 32-byte raw private scalar, base64url-no-pad.
    pub private_key_b64: String,
    /// 65-byte uncompressed public point (0x04 + X + Y),
    /// base64url-no-pad — this is what the browser binds.
    pub public_key_b64: String,
    /// mailto: contact the push service uses to reach us on abuse.
    pub subject: String,
}

pub fn config_from_env() -> Option<VapidConfig> {
    let priv_b64 = std::env::var("VAPID_PRIVATE_KEY").ok()?;
    let pub_b64 = std::env::var("VAPID_PUBLIC_KEY").ok()?;
    let subject = std::env::var("VAPID_SUBJECT")
        .unwrap_or_else(|_| "mailto:admin@justyeet.it".into());
    if priv_b64.is_empty() || pub_b64.is_empty() {
        return None;
    }
    Some(VapidConfig {
        private_key_b64: priv_b64,
        public_key_b64: pub_b64,
        subject,
    })
}

/// Sign a VAPID JWT for the given push-service origin and return the
/// `Authorization: vapid t=<jwt>, k=<vapid_public_key>` header value
/// that goes on every push request.
pub fn make_vapid_header(cfg: &VapidConfig, endpoint: &str) -> anyhow::Result<String> {
    let origin = origin_of(endpoint).ok_or_else(|| anyhow::anyhow!("bad endpoint"))?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).as_secs();
    // RFC 8292 §2: aud = push service origin, exp <= now + 24h.
    let claims = VapidClaims {
        aud: origin,
        exp: now as i64 + 12 * 3600,
        sub: cfg.subject.clone(),
    };

    let header_json = r#"{"typ":"JWT","alg":"ES256"}"#;
    let payload_json = serde_json::to_string(&claims)?;
    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header_json),
        URL_SAFE_NO_PAD.encode(payload_json),
    );

    let priv_bytes = URL_SAFE_NO_PAD.decode(&cfg.private_key_b64)
        .map_err(|e| anyhow::anyhow!("VAPID private key b64 decode: {e}"))?;
    if priv_bytes.len() != 32 {
        anyhow::bail!("VAPID private key must be exactly 32 bytes");
    }
    let signing_key = SigningKey::from_bytes(priv_bytes.as_slice().into())
        .map_err(|e| anyhow::anyhow!("VAPID private key parse: {e}"))?;
    let sig: Signature = signing_key.sign(signing_input.as_bytes());

    // JWS ES256 signature is r||s, 64 bytes total (each 32 bytes).
    // p256's Signature::to_bytes() returns exactly that.
    let sig_bytes = sig.to_bytes();
    let jwt = format!("{}.{}", signing_input, URL_SAFE_NO_PAD.encode(sig_bytes));

    Ok(format!("vapid t={}, k={}", jwt, cfg.public_key_b64))
}

#[derive(Serialize, Deserialize, Debug)]
struct VapidClaims {
    aud: String,
    exp: i64,
    sub: String,
}

fn origin_of(url: &str) -> Option<String> {
    // Cheap origin extractor — we don't pull in `url` for one call.
    // Format: scheme://host[:port][/path...]
    let proto_end = url.find("://")?;
    let after_proto = &url[proto_end + 3..];
    let host_end = after_proto.find('/').unwrap_or(after_proto.len());
    Some(format!("{}{}{}", &url[..proto_end], "://", &after_proto[..host_end]))
}

/// Send a single tickle push. Returns `Ok(true)` on success,
/// `Ok(false)` if the subscription is gone (caller should mark
/// expired_at), `Err` for transient errors.
pub async fn send_tickle(
    cfg: &VapidConfig,
    endpoint: &str,
    client: &reqwest::Client,
) -> anyhow::Result<bool> {
    let auth = make_vapid_header(cfg, endpoint)?;
    // The body is empty — no encrypted payload, no Content-Encoding.
    // Per RFC 8030 the TTL header is still required.
    let resp = client.post(endpoint)
        .header("Authorization", auth)
        .header("TTL", "60")
        .header("Content-Length", "0")
        .send().await?;

    if resp.status().is_success() {
        return Ok(true);
    }
    if resp.status() == 404 || resp.status() == 410 {
        // Subscription is gone — caller should mark it expired.
        return Ok(false);
    }
    anyhow::bail!("push service returned {}", resp.status());
}

/// Compute the JS-side `applicationServerKey` representation for a
/// VAPID public key. Browsers want the 65-byte uncompressed point
/// as a `Uint8Array`; we already store it as base64url-no-pad which
/// is what the client will decode.
pub fn application_server_key_b64(cfg: &VapidConfig) -> &str {
    &cfg.public_key_b64
}

#[allow(dead_code)]
pub fn sha256_b64(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(data))
}
