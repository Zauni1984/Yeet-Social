//! Retry with exponential backoff for transient failures.

use std::future::Future;
use std::time::Duration;
use tokio::time::sleep;

use crate::error::{NetworkError, NetworkResult};

/// Configuration for retry behaviour.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts (0 = no retries).
    pub max_retries: u32,
    /// Initial delay before the first retry.
    pub base_delay: Duration,
    /// Maximum delay cap (backoff won't exceed this).
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(10),
        }
    }
}

/// Whether an error is worth retrying.
#[must_use]
pub fn is_retryable(err: &NetworkError) -> bool {
    matches!(
        err,
        NetworkError::Timeout
            | NetworkError::Connection(_)
            | NetworkError::Http { status: 429, .. }
            | NetworkError::Http { status: 502, .. }
            | NetworkError::Http { status: 503, .. }
            | NetworkError::Http { status: 504, .. }
    )
}

/// Execute an async operation with retry.
///
/// Retries only on transient errors (timeout, connection, 429, 5xx).
/// Non-retryable errors are returned immediately.
///
/// # Errors
/// Returns the last error after all retries are exhausted.
pub async fn with_retry<F, Fut, T>(policy: &RetryPolicy, mut operation: F) -> NetworkResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = NetworkResult<T>>,
{
    let mut attempt = 0;

    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(err) if attempt < policy.max_retries && is_retryable(&err) => {
                attempt += 1;
                let delay = policy
                    .base_delay
                    .saturating_mul(2u32.saturating_pow(attempt - 1))
                    .min(policy.max_delay);

                tracing::warn!(
                    attempt,
                    max = policy.max_retries,
                    delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                    error = %err,
                    "retrying after transient error"
                );

                sleep(delay).await;
            }
            Err(err) => return Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_is_retryable() {
        assert!(is_retryable(&NetworkError::Timeout));
    }

    #[test]
    fn rpc_error_is_not_retryable() {
        assert!(!is_retryable(&NetworkError::Rpc {
            code: -32600,
            message: "bad request".into(),
        }));
    }

    #[test]
    fn rate_limit_429_is_retryable() {
        assert!(is_retryable(&NetworkError::Http {
            status: 429,
            body: "too many".into(),
        }));
    }

    #[test]
    fn client_400_is_not_retryable() {
        assert!(!is_retryable(&NetworkError::Http {
            status: 400,
            body: "bad request".into(),
        }));
    }

    #[tokio::test]
    async fn succeeds_on_first_try() {
        let policy = RetryPolicy {
            max_retries: 3,
            ..RetryPolicy::default()
        };
        let result = with_retry(&policy, || async { Ok::<_, NetworkError>(42) }).await;
        assert_eq!(result.expect("should succeed"), 42);
    }

    #[tokio::test]
    async fn gives_up_after_max_retries() {
        let policy = RetryPolicy {
            max_retries: 2,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(5),
        };
        let mut attempt = 0u32;
        let result = with_retry(&policy, || {
            attempt += 1;
            async { Err::<i32, _>(NetworkError::Timeout) }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(attempt, 3); // 1 initial + 2 retries
    }
}

// Rust guideline compliant 2026-05-02
