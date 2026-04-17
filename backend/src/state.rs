//! Shared application state injected into every handler.
use anyhow::{Context, Result};
use tracing::info;
use crate::db::Database;
use crate::services::cache::Cache;
use crate::services::email::EmailConfig;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct JwtConfig {
    pub secret: String,
    pub access_ttl_secs: u64,
    pub refresh_ttl_secs: u64,
}

#[derive(Clone, Debug)]
pub struct BlockchainConfig {
    pub rpc_url: String,
    pub token_address: String,
    pub nft_address: String,
    pub minter_privkey: String,
}

#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub cache: Cache,
    pub jwt: JwtConfig,
    pub blockchain: BlockchainConfig,
    pub email: Option<Arc<EmailConfig>>,
}

impl AppState {
    pub async fn from_env() -> Result<Self> {
        let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL required")?;
        let db = Database::connect(&database_url).await?;
        info!("✅ PostgreSQL connected");

        let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".into());
        let cache = Cache::connect(&redis_url).await?;
        info!("✅ Redis connected");

        let jwt_secret = std::env::var("JWT_SECRET").context("JWT_SECRET required")?;
        if jwt_secret.len() < 32 { anyhow::bail!("JWT_SECRET must be >= 32 chars"); }

        let jwt = JwtConfig {
            secret: jwt_secret,
            access_ttl_secs: env_u64("JWT_ACCESS_TTL_SECS", 3600),
            refresh_ttl_secs: env_u64("JWT_REFRESH_TTL_SECS", 604_800),
        };

        let blockchain = BlockchainConfig {
            rpc_url:        std::env::var("BSC_RPC_URL").unwrap_or_else(|_| "https://bsc-dataseed.binance.org/".into()),
            token_address:  std::env::var("YEET_TOKEN_ADDRESS").unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".into()),
            nft_address:    std::env::var("YEET_NFT_ADDRESS").unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".into()),
            minter_privkey: std::env::var("REWARDS_MINTER_PRIVKEY").unwrap_or_default(),
        };

        let email = EmailConfig::from_env().map(Arc::new);
        if let Some(cfg) = &email {
            info!("✅ SMTP configured ({})", cfg.host);
        } else {
            info!("⚠  SMTP not configured — email verification will fail silently");
        }

        Ok(Self { db, cache, jwt, blockchain, email })
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}
