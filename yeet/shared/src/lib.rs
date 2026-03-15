use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── User ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub bio: Option<String>,
    pub avatar_url: Option<String>,
    pub wallet_address: Option<String>, // BSC wallet
    pub country_code: Option<String>,
    pub is_verified: bool,
    pub age_verified: bool, // for 18+ content
    pub yeet_token_balance: f64,
    pub created_at: DateTime<Utc>,
}

// ─── Post ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PostVisibility {
    Public,
    FollowersOnly,
    AgeRestricted, // 18+
    PayPerView,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PostSource {
    Yeet,             // native post
    WebBoard(String), // external forum/board domain
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Post {
    pub id: Uuid,
    pub author_id: Uuid,
    pub author_username: String,
    pub content: String,
    pub media_urls: Vec<String>,
    pub visibility: PostVisibility,
    pub source: PostSource,
    pub pay_per_view_price: Option<f64>, // in YEET tokens
    pub is_nft: bool,
    pub nft_token_id: Option<String>,   // BSC token ID if minted
    pub nft_contract: Option<String>,   // contract address
    pub like_count: i64,
    pub comment_count: i64,
    pub reshare_count: i64,
    pub tip_total: f64, // total YEET tips received
    pub expires_at: DateTime<Utc>,      // 24h from last reshare
    pub created_at: DateTime<Utc>,
    pub reshared_from: Option<Uuid>,
}

// ─── Comment ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: Uuid,
    pub post_id: Uuid,
    pub author_id: Uuid,
    pub author_username: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

// ─── Tip ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TipCurrency {
    Yeet,
    Bnb,
    Fiat, // PayPal etc.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tip {
    pub id: Uuid,
    pub from_user_id: Uuid,
    pub to_user_id: Uuid,
    pub post_id: Uuid,
    pub amount: f64,
    pub currency: TipCurrency,
    pub tx_hash: Option<String>, // BSC tx hash
    pub created_at: DateTime<Utc>,
}

// ─── Token Reward ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RewardAction {
    DailyLogin,
    Comment,
    Share,
    Reshare,
    Downvote,
    MintNft,
    ReferralSignup,
}

impl RewardAction {
    /// Token reward amount per action (YEET)
    pub fn reward_amount(&self) -> f64 {
        match self {
            RewardAction::DailyLogin => 1.0,
            RewardAction::Comment => 0.5,
            RewardAction::Share => 0.5,
            RewardAction::Reshare => 0.25,
            RewardAction::Downvote => 0.1,
            RewardAction::MintNft => 5.0,
            RewardAction::ReferralSignup => 10.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenReward {
    pub id: Uuid,
    pub user_id: Uuid,
    pub action: RewardAction,
    pub amount: f64,
    pub tx_hash: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ─── Feed ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FeedMode {
    Global,
    Following,
    Subscriptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedRequest {
    pub mode: FeedMode,
    pub show_18_plus: bool,
    pub cursor: Option<DateTime<Utc>>, // pagination
    pub limit: i64,
}

// ─── Subscription / Membership ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MembershipTier {
    Free,
    Weekly,
    Monthly,
    Yearly,
}

impl MembershipTier {
    /// Price in EUR (half of OnlyFans)
    pub fn price_eur(&self) -> f64 {
        match self {
            MembershipTier::Free => 0.0,
            MembershipTier::Weekly => 2.49,
            MembershipTier::Monthly => 4.99,
            MembershipTier::Yearly => 39.99,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub id: Uuid,
    pub subscriber_id: Uuid,
    pub creator_id: Uuid,
    pub tier: MembershipTier,
    pub valid_until: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

// ─── API responses ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self { success: true, data: Some(data), error: None }
    }
    pub fn err(msg: impl Into<String>) -> Self {
        Self { success: false, data: None, error: Some(msg.into()) }
    }
}
