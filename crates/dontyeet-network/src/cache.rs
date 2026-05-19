//! TTL-based response cache.
//!
//! Avoids hammering RPC nodes for identical queries within short windows
//! (e.g. repeated balance checks, fee estimations).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A time-to-live cache for arbitrary cloneable values.
pub struct TtlCache<V> {
    entries: Mutex<HashMap<String, CacheEntry<V>>>,
    default_ttl: Duration,
}

struct CacheEntry<V> {
    value: V,
    expires_at: Instant,
}

impl<V: Clone> TtlCache<V> {
    /// Create a new cache with the given default TTL.
    #[must_use]
    pub fn new(default_ttl: Duration) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            default_ttl,
        }
    }

    /// Get a cached value if it exists and hasn't expired.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<V> {
        let entries = self.entries.lock().ok()?;
        let entry = entries.get(key)?;
        if Instant::now() < entry.expires_at {
            Some(entry.value.clone())
        } else {
            None
        }
    }

    /// Insert a value with the default TTL.
    pub fn set(&self, key: String, value: V) {
        self.set_with_ttl(key, value, self.default_ttl);
    }

    /// Insert a value with a custom TTL.
    pub fn set_with_ttl(&self, key: String, value: V, ttl: Duration) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(
                key,
                CacheEntry {
                    value,
                    expires_at: Instant::now() + ttl,
                },
            );
        }
    }

    /// Remove a specific key.
    pub fn invalidate(&self, key: &str) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.remove(key);
        }
    }

    /// Remove all expired entries.
    pub fn evict_expired(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            let now = Instant::now();
            entries.retain(|_, entry| entry.expires_at > now);
        }
    }

    /// Remove all entries.
    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_returns_cached_value() {
        let cache = TtlCache::new(Duration::from_secs(60));
        cache.set("key".into(), 42);
        assert_eq!(cache.get("key"), Some(42));
    }

    #[test]
    fn get_returns_none_for_missing() {
        let cache: TtlCache<i32> = TtlCache::new(Duration::from_secs(60));
        assert_eq!(cache.get("nope"), None);
    }

    #[test]
    fn expired_entry_returns_none() {
        let cache = TtlCache::new(Duration::from_millis(1));
        cache.set("key".into(), 42);
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(cache.get("key"), None);
    }

    #[test]
    fn invalidate_removes_entry() {
        let cache = TtlCache::new(Duration::from_secs(60));
        cache.set("key".into(), 42);
        cache.invalidate("key");
        assert_eq!(cache.get("key"), None);
    }

    #[test]
    fn clear_removes_all() {
        let cache = TtlCache::new(Duration::from_secs(60));
        cache.set("a".into(), 1);
        cache.set("b".into(), 2);
        cache.clear();
        assert_eq!(cache.get("a"), None);
        assert_eq!(cache.get("b"), None);
    }
}

// Rust guideline compliant 2026-05-02
