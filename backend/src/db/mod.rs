use anyhow::Result;
use sqlx::{PgPool, postgres::PgPoolOptions};

/// Thin wrapper around sqlx PgPool
#[derive(Clone)]
pub struct Database {
    pub pool: PgPool,
}

pub type RedisPool = deadpool_redis::Pool;

impl Database {
    pub async fn connect(url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(20)
            .connect(url)
            .await?;
        Ok(Self { pool })
    }

    /// Run embedded migrations from ./migrations/
    pub async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }
}

impl RedisPool {
    pub fn new(url: &str) -> Result<Self> {
        let cfg = deadpool_redis::Config::from_url(url);
        let pool = cfg.create_pool(Some(deadpool_redis::Runtime::Tokio1))?;
        Ok(pool)
    }
}
