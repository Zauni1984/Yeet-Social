//! Token-bucket rate limiter to avoid hitting RPC provider limits.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::{NetworkError, NetworkResult};

/// Token-bucket rate limiter.
///
/// Tokens refill at a constant rate up to a maximum capacity.
/// Each request consumes one token.  If no tokens are available,
/// the request is rejected with [`NetworkError::RateLimited`].
pub struct TokenBucket {
    capacity: u32,
    refill_rate_per_sec: f64,
    state: Mutex<BucketState>,
}

struct BucketState {
    tokens: f64,
    last_refill: Instant,
}

impl TokenBucket {
    /// Create a new rate limiter.
    ///
    /// - `capacity`: maximum burst size (tokens).
    /// - `refill_rate_per_sec`: how many tokens refill per second.
    #[must_use]
    pub fn new(capacity: u32, refill_rate_per_sec: f64) -> Self {
        Self {
            capacity,
            refill_rate_per_sec,
            state: Mutex::new(BucketState {
                tokens: f64::from(capacity),
                last_refill: Instant::now(),
            }),
        }
    }

    /// Try to acquire one token.  Returns `Ok(())` if allowed,
    /// `Err(RateLimited)` if the bucket is empty.
    ///
    /// # Errors
    /// Returns `NetworkError::RateLimited` if no tokens are available.
    pub fn try_acquire(&self) -> NetworkResult<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|e| NetworkError::Connection(format!("rate limiter lock poisoned: {e}")))?;

        let now = Instant::now();
        let elapsed = now.duration_since(state.last_refill).as_secs_f64();
        state.tokens =
            (state.tokens + elapsed * self.refill_rate_per_sec).min(f64::from(self.capacity));
        state.last_refill = now;

        if state.tokens >= 1.0 {
            state.tokens -= 1.0;
            Ok(())
        } else {
            Err(NetworkError::RateLimited)
        }
    }

    /// Blocking acquire: wait until a token is available.
    pub async fn acquire(&self) {
        loop {
            if self.try_acquire().is_ok() {
                return;
            }
            // Wait a small interval before retrying.
            let wait = Duration::from_secs_f64(1.0 / self.refill_rate_per_sec);
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_allowed() {
        let bucket = TokenBucket::new(3, 1.0);
        assert!(bucket.try_acquire().is_ok());
        assert!(bucket.try_acquire().is_ok());
        assert!(bucket.try_acquire().is_ok());
    }

    #[test]
    fn exhausted_bucket_rejects() {
        let bucket = TokenBucket::new(1, 1.0);
        assert!(bucket.try_acquire().is_ok());
        assert!(bucket.try_acquire().is_err());
    }

    #[tokio::test]
    async fn refills_over_time() {
        let bucket = TokenBucket::new(1, 100.0); // 100 tokens/sec = fast refill
        assert!(bucket.try_acquire().is_ok());
        // Exhausted
        assert!(bucket.try_acquire().is_err());
        // Wait for refill
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(bucket.try_acquire().is_ok());
    }
}

// Rust guideline compliant 2026-05-02
