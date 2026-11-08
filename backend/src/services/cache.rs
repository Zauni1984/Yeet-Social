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

    /// `scope` separates the login challenge from the wallet-link challenge
    /// (audit A-M6): a nonce issued for one flow can never be consumed by
    /// the other.
    pub async fn set_nonce(&self, scope: &str, address: &str, nonce: &str, ttl: Duration) -> Result<()> {
        let mut c = self.conn.clone();
        c.set_ex::<_, _, ()>(format!("nonce:{scope}:{}", address.to_lowercase()), nonce, ttl.as_secs())
            .await.context("Failed to set nonce")?;
        Ok(())
    }

    pub async fn consume_nonce(&self, scope: &str, address: &str) -> Result<Option<String>> {
        let mut c = self.conn.clone();
        let v: Option<String> = c.get_del(format!("nonce:{scope}:{}", address.to_lowercase()))
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

    /// Audit A-M1: mark a whole session family revoked so every access token
    /// minted for it is rejected at once (TTL = access-token lifetime).
    pub async fn revoke_family(&self, family_id: &str, ttl: Duration) -> Result<()> {
        let mut c = self.conn.clone();
        c.set_ex::<_, _, ()>(format!("revoked_sid:{family_id}"), "1", ttl.as_secs())
            .await.context("Failed to revoke family")?;
        Ok(())
    }

    /// Reject every access token of a subject (account deletion).
    pub async fn revoke_subject(&self, sub: &str, ttl: Duration) -> Result<()> {
        let mut c = self.conn.clone();
        c.set_ex::<_, _, ()>(format!("revoked_sub:{}", sub.to_lowercase()), "1", ttl.as_secs())
            .await.context("Failed to revoke subject")?;
        Ok(())
    }

    /// One round trip: is the token's JTI blacklisted, its session family
    /// revoked, or its subject revoked?
    pub async fn is_revoked(&self, jti: &str, sid: Option<&str>, sub: &str) -> Result<bool> {
        let mut c = self.conn.clone();
        let mut keys = vec![format!("blacklist:{jti}"), format!("revoked_sub:{}", sub.to_lowercase())];
        if let Some(s) = sid { keys.push(format!("revoked_sid:{s}")); }
        let n: i64 = c.exists(keys).await.context("Failed to check revocation")?;
        Ok(n > 0)
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
        // Refresh the TTL on every hit, not just the first. If the process
        // died between INCR and EXPIRE on the n==1 path, the key could
        // otherwise live forever with no TTL — a permanent rate-limit
        // lockout for that key. Re-arming each call is idempotent and cheap.
        c.expire::<_, ()>(key, ttl.as_secs() as i64).await.context("Failed to set TTL")?;
        Ok(n)
    }
}
