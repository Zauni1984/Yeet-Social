#![allow(dead_code)]
//! Domain models used across API, services, and DB.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub wallet_address: Option<String>,
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub avatar_url: Option<String>,
    pub adult_content: bool,
    pub total_yeet_earned: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserProfile {
    pub id: Uuid,
    pub wallet_address: Option<String>,
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub avatar_url: Option<String>,
    pub cover_url: Option<String>,
    pub follower_count: i64,
    pub following_count: i64,
    pub post_count: i64,
    pub age_verified: bool,
    pub created_at: DateTime<Utc>,
    // True iff the caller has blocked this user. Defaults to false for
    // anonymous callers.
    #[serde(default)]
    pub is_blocked_by_me: bool,
    // True iff this user has blocked the caller.
    #[serde(default)]
    pub has_blocked_me: bool,
    // True iff this user has uploaded an E2EE identity public key
    // (i.e. is reachable for encrypted DMs).
    #[serde(default)]
    pub e2ee_ready: bool,
    // True iff the caller follows this user. Defaults to false for
    // anonymous callers.
    #[serde(default)]
    pub is_following: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Post {
    pub id: Uuid,
    pub author_id: Uuid,
    pub content: String,
    pub media_url: Option<String>,
    pub is_adult: bool,
    pub nft_token_id: Option<i64>,
    pub nft_metadata_uri: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub like_count: i32,
    pub reshare_count: i32,
    pub comment_count: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FeedPost {
    pub id: Uuid,
    pub content: String,
    pub media_url: Option<String>,
    pub is_adult: bool,
    pub is_nft: bool,
    pub like_count: i32,
    pub reshare_count: i32,
    pub comment_count: i32,
    pub is_liked: bool,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub author: FeedPostAuthor,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tip_total_yeet: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nft_price_yeet: Option<f64>,
    pub is_permanent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ppv_price_yeet: Option<f64>,
    /// True iff the calling viewer has paid to unlock this post. Always
    /// true for the post's own author. NULL ppv_price_yeet posts also
    /// report true (no unlock needed). Anonymous viewers always false.
    #[serde(default)]
    pub is_unlocked: bool,
    /// If this post is a reshare/repost wrapper, the id + display info
    /// of the *original* post's author so the UI can label it as such.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reposted_from: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reposted_from_author_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reposted_from_author_username: Option<String>,
    /// If set, this post is an auto-promo for the given live broadcast.
    /// The client renders a "Watch live" CTA instead of plain text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promoted_live_id: Option<Uuid>,
    /// Boost-tier promotions pin a post until this timestamp; feed
    /// queries sort pinned posts above everything else until expiry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned_until: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FeedPostAuthor {
    pub id: Uuid,
    pub wallet_address: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Comment {
    pub id: Uuid,
    pub post_id: Uuid,
    pub author_id: Uuid,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub success: bool,
    pub data: T,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn ok(data: T) -> Self { Self { success: true, data } }
}

#[derive(Debug, Serialize)]
pub struct PagedResponse<T: Serialize> {
    pub success: bool,
    pub data: Vec<T>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}
