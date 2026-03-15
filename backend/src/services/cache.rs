//! Redis cache service — sessions, nonces, rate limiting.
use anyhow::{Context, Result};
use redis::{aio::ConnectionManager, AsyncCommands, Client};
use std::time::Duration;

#[derive(Clone)]
pub struct Cache { conn: ConnectionManager }

impl Cache {
    pub async fn connect(url: &str) -> Result<Self> {
        let client = Client::open(url).context("Invalid Redis URL")?;
        let conn = ConnectionManager::new(client).await.context("Redis connect failed")?;
        Ok(Self { conn })
    }

    pub async fn ping(&self) -> Result<()> {
        let mut c = self.conn.clone();
        let _: String = redis::cmd("PING").query_async(&mut c).await.context("Redis ping failed")?;
        Ok(())
    }

    pub async fn set_nonce(&self, address: &str, nonce: &str, ttl: Duration) -> Result<()> {
        let mut c = self.conn.clone();
        c.set_ex::<_, _, ()>(format!("nonce:{}", address.to_lowercase()), nonce, ttl.as_secs())
            .await.context("Failed to set nonce")?;
        Ok(())
    }

    pub async fn consume_nonce(&self, address: &str) -> Result<Option<String>> {
        let mut c = self.conn.clone();
        let v: Option<String> = c.get_del(format!("nonce:{}", address.to_lowercase()))
            .await.context("Failed to consume nonce")?;
        Ok(v)
    }

    pub async fn blacklist_token(&self, jti: &str, ttl: Duration) -> Result<()> {
        let mut c = self.conn.clone();
        c.set_ex::<_, _, ()>(format!("blacklist:{jti}"), "1", ttl.as_secs())
            .await.context("Failed to blacklist token")?;
        Ok(())
    }

    pub async fn is_blacklisted(&self, jti: &str) -> Result<bool> {
        let mut c = self.conn.clone();
        let e: bool = c.exists(format!("blacklist:{jti}")).await.context("Failed to check blacklist")?;
        Ok(e)
    }

    pub async fn get(&self, key: &str) -> Result<Option<String>> {
        let mut c = self.conn.clone();
        let v: Option<String> = c.get(key).await.context("Failed to get")?;
        Ok(v)
    }

    pub async fn set_ttl(&self, key: &str, value: &str, ttl: Duration) -> Result<()> {
        let mut c = self.conn.clone();
        c.set_ex::<_, _, ()>(key, value, ttl.as_secs()).await.context("Failed to set")?;
        Ok(())
    }

    pub async fn incr(&self, key: &str, ttl: Duration) -> Result<i64> {
        let mut c = self.conn.clone();
        let n: i64 = c.incr(key, 1).await.context("Failed to incr")?;
        if n == 1 { c.expire::<_, ()>(key, ttl.as_secs() as i64).await.context("Failed to set TTL")?; }
        Ok(n)
    }
}
