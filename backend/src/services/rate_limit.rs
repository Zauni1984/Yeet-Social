//! Redis-backed rate limiter for messaging-adjacent endpoints.
//!
//! Implements the classic fixed-window counter (INCR + EXPIRE) using
//! the existing `Cache::incr` primitive. Two windows let callers
//! enforce both burst protection (small N over a few seconds) and
//! sustained-volume protection (larger N over an hour) with a single
//! call. Returning a structured `RateLimitOutcome` instead of bool
//! makes the call sites clearly say "what got exceeded" in error
//! messages without leaking the actual counts to clients.
//!
//! Key naming convention:
//!   rl:<scope>:<bucket>:<window_secs>:<period_id>
//!
//! `period_id` is the wall-clock bucket index (Unix-time / window),
//! which means the windows reset on absolute boundaries rather than
//! per-caller — this is fine for abuse mitigation and makes the math
//! free of any clock-state on the server.
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::services::cache::Cache;

/// What got tripped (if anything). `Allowed` means both checks passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitOutcome {
    Allowed,
    /// Short-window cap was tripped — caller should hold off briefly.
    Burst,
    /// Sustained-window cap was tripped — caller is over their hourly /
    /// daily envelope.
    Sustained,
}

/// Two-window check: increments both counters in lock-step. If either
/// would exceed its cap the call is rejected — and importantly we still
/// burn an INCR on both so the limiter is robust to attackers
/// hammering a single window once they spot it.
///
/// `scope` is a short, namespace-stable string (e.g. "msg_send"). The
/// other args identify the principal we're throttling.
pub async fn check_two_window(
    cache: &Cache,
    scope: &str,
    principal: &str,
    burst_window_secs: u64,
    burst_cap: i64,
    sustained_window_secs: u64,
    sustained_cap: i64,
) -> RateLimitOutcome {
    let burst = incr_window(cache, scope, principal, burst_window_secs).await;
    let sustained = incr_window(cache, scope, principal, sustained_window_secs).await;
    if burst > burst_cap {
        return RateLimitOutcome::Burst;
    }
    if sustained > sustained_cap {
        return RateLimitOutcome::Sustained;
    }
    RateLimitOutcome::Allowed
}

/// Single-window variant for endpoints where one cap is enough
/// (e.g. key uploads — a few per hour, no burst dimension to worry).
pub async fn check_one_window(
    cache: &Cache,
    scope: &str,
    principal: &str,
    window_secs: u64,
    cap: i64,
) -> bool {
    let n = incr_window(cache, scope, principal, window_secs).await;
    n <= cap
}

async fn incr_window(cache: &Cache, scope: &str, principal: &str, window_secs: u64) -> i64 {
    let bucket = current_bucket(window_secs);
    let key = format!("rl:{scope}:{principal}:{window_secs}:{bucket}");
    // Redis is best-effort here: if it's down we fail-open. Rate limit
    // is a depth-of-defence; auth + DB-level uniqueness keep the actual
    // invariants safe. This avoids cascading a Redis outage into a
    // full-app outage.
    cache.incr(&key, Duration::from_secs(window_secs + 5)).await.unwrap_or(0)
}

fn current_bucket(window_secs: u64) -> u64 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    now.checked_div(window_secs).unwrap_or(0)
}
