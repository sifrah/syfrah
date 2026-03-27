//! Token-bucket rate limiter keyed by API key name.
//!
//! Each API key gets its own bucket with a sustained rate of 100 req/s and a
//! burst capacity of 200 tokens. There is also an aggregate (all-keys) bucket
//! capped at 1 000 req/s.
//!
//! When a request is rejected the caller receives a `RESOURCE_EXHAUSTED` error
//! and a `retry_after_ms` hint indicating how long to wait before retrying.

use std::collections::HashMap;
use std::time::Instant;

use crate::error::{ApiError, RESOURCE_EXHAUSTED};

// ---------------------------------------------------------------------------
// Configuration defaults
// ---------------------------------------------------------------------------

/// Default sustained rate per API key (tokens added per second).
const DEFAULT_RATE_PER_SEC: f64 = 100.0;

/// Default burst capacity per API key.
const DEFAULT_BURST: u32 = 200;

/// Default aggregate rate across all keys.
const DEFAULT_AGGREGATE_RATE_PER_SEC: f64 = 1000.0;

/// Default aggregate burst.
const DEFAULT_AGGREGATE_BURST: u32 = 2000;

// ---------------------------------------------------------------------------
// RateLimitResponse — extra metadata on rejection
// ---------------------------------------------------------------------------

/// Returned when a request is rejected so the caller can set headers.
#[derive(Debug, Clone)]
pub struct RateLimitRejection {
    /// The `ApiError` to return to the client.
    pub error: ApiError,
    /// Suggested retry delay in milliseconds (for the `Retry-After` header).
    pub retry_after_ms: u64,
}

// ---------------------------------------------------------------------------
// TokenBucket
// ---------------------------------------------------------------------------

/// Classic token-bucket: tokens refill at `rate` per second, up to `capacity`.
#[derive(Debug, Clone)]
struct TokenBucket {
    /// Maximum number of tokens the bucket can hold.
    capacity: u32,
    /// Current token count (fractional to support sub-second refill).
    tokens: f64,
    /// Tokens added per second.
    rate: f64,
    /// When we last refilled.
    last_refill: Instant,
}

impl TokenBucket {
    fn new(rate: f64, capacity: u32) -> Self {
        Self {
            capacity,
            tokens: capacity as f64,
            rate,
            last_refill: Instant::now(),
        }
    }

    fn new_at(rate: f64, capacity: u32, now: Instant) -> Self {
        Self {
            capacity,
            tokens: capacity as f64,
            rate,
            last_refill: now,
        }
    }

    /// Try to consume one token. Returns `Ok(())` on success, or
    /// `Err(retry_after_ms)` indicating how long until a token is available.
    fn try_consume(&mut self, now: Instant) -> Result<(), u64> {
        self.refill(now);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Ok(())
        } else {
            // Time until one token is available.
            let deficit = 1.0 - self.tokens;
            let wait_secs = deficit / self.rate;
            let wait_ms = (wait_secs * 1000.0).ceil() as u64;
            Err(wait_ms.max(1))
        }
    }

    fn refill(&mut self, now: Instant) {
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        if elapsed > 0.0 {
            self.tokens = (self.tokens + elapsed * self.rate).min(self.capacity as f64);
            self.last_refill = now;
        }
    }
}

// ---------------------------------------------------------------------------
// RateLimiter
// ---------------------------------------------------------------------------

/// Per-instance rate limiter that tracks token buckets per API key name and
/// an aggregate bucket across all keys.
pub struct RateLimiter {
    buckets: HashMap<String, TokenBucket>,
    aggregate: TokenBucket,
    per_key_rate: f64,
    per_key_burst: u32,
}

impl RateLimiter {
    /// Create a new `RateLimiter` with default settings.
    pub fn new() -> Self {
        Self {
            buckets: HashMap::new(),
            aggregate: TokenBucket::new(DEFAULT_AGGREGATE_RATE_PER_SEC, DEFAULT_AGGREGATE_BURST),
            per_key_rate: DEFAULT_RATE_PER_SEC,
            per_key_burst: DEFAULT_BURST,
        }
    }

    /// Check whether a request from the given API key should be allowed.
    ///
    /// Returns `Ok(())` if the request may proceed, or `Err(RateLimitRejection)`
    /// if the key (or aggregate) has exceeded its limit.
    pub fn check(&mut self, key_name: &str) -> Result<(), RateLimitRejection> {
        self.check_at(key_name, Instant::now())
    }

    /// Same as [`check`](Self::check) but accepts an explicit timestamp
    /// (useful for deterministic testing).
    fn check_at(&mut self, key_name: &str, now: Instant) -> Result<(), RateLimitRejection> {
        // Check aggregate first.
        if let Err(retry_ms) = self.aggregate.try_consume(now) {
            return Err(RateLimitRejection {
                error: ApiError::new(RESOURCE_EXHAUSTED, "aggregate rate limit exceeded"),
                retry_after_ms: retry_ms,
            });
        }

        // Check per-key bucket.
        let rate = self.per_key_rate;
        let burst = self.per_key_burst;
        let bucket = self
            .buckets
            .entry(key_name.to_string())
            .or_insert_with(|| TokenBucket::new_at(rate, burst, now));

        if let Err(retry_ms) = bucket.try_consume(now) {
            return Err(RateLimitRejection {
                error: ApiError::new(
                    RESOURCE_EXHAUSTED,
                    format!("rate limit exceeded for key '{key_name}'"),
                ),
                retry_after_ms: retry_ms,
            });
        }

        Ok(())
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn allows_requests_within_burst() {
        let now = Instant::now();
        let mut limiter = RateLimiter::new();

        // Should allow up to DEFAULT_BURST (200) requests immediately.
        for i in 0..DEFAULT_BURST {
            assert!(
                limiter.check_at("test-key", now).is_ok(),
                "request {i} should be allowed"
            );
        }
    }

    #[test]
    fn rejects_after_burst_exhausted() {
        let now = Instant::now();
        let mut limiter = RateLimiter::new();

        // Drain the burst.
        for _ in 0..DEFAULT_BURST {
            limiter.check_at("test-key", now).unwrap();
        }

        // Next request should be rejected.
        let result = limiter.check_at("test-key", now);
        assert!(result.is_err(), "should reject after burst exhausted");

        let rejection = result.unwrap_err();
        assert_eq!(rejection.error.code, RESOURCE_EXHAUSTED);
        assert!(rejection.retry_after_ms > 0);
    }

    #[test]
    fn refills_over_time() {
        let now = Instant::now();
        let mut limiter = RateLimiter::new();

        // Drain the burst.
        for _ in 0..DEFAULT_BURST {
            limiter.check_at("test-key", now).unwrap();
        }

        // Rejected immediately.
        assert!(limiter.check_at("test-key", now).is_err());

        // Advance 1 second: should have refilled 100 tokens.
        let later = now + Duration::from_secs(1);
        for i in 0..100 {
            assert!(
                limiter.check_at("test-key", later).is_ok(),
                "request {i} after refill should be allowed"
            );
        }

        // 101st request at the same instant should be rejected.
        assert!(limiter.check_at("test-key", later).is_err());
    }

    #[test]
    fn per_key_isolation() {
        let now = Instant::now();
        let mut limiter = RateLimiter::new();

        // Drain key A.
        for _ in 0..DEFAULT_BURST {
            limiter.check_at("key-a", now).unwrap();
        }
        assert!(limiter.check_at("key-a", now).is_err());

        // Key B should still have its full burst.
        assert!(limiter.check_at("key-b", now).is_ok());
    }

    #[test]
    fn aggregate_limit_enforced() {
        let now = Instant::now();
        let mut limiter = RateLimiter::new();

        // Burn through aggregate bucket (2000) using many different keys so
        // no per-key limit is hit.
        for i in 0..DEFAULT_AGGREGATE_BURST {
            let key = format!("key-{i}");
            assert!(
                limiter.check_at(&key, now).is_ok(),
                "aggregate request {i} should be allowed"
            );
        }

        // Next request on a fresh key should be rejected by aggregate.
        let result = limiter.check_at("key-fresh", now);
        assert!(result.is_err(), "should hit aggregate limit");
        let rejection = result.unwrap_err();
        assert!(rejection.error.message.contains("aggregate"));
    }

    #[test]
    fn retry_after_is_reasonable() {
        let now = Instant::now();
        let mut limiter = RateLimiter::new();

        for _ in 0..DEFAULT_BURST {
            limiter.check_at("key-r", now).unwrap();
        }

        let rejection = limiter.check_at("key-r", now).unwrap_err();
        // At 100 tokens/s, one token takes 10 ms.
        assert!(
            rejection.retry_after_ms <= 11,
            "retry_after_ms should be ~10, got {}",
            rejection.retry_after_ms
        );
        assert!(rejection.retry_after_ms >= 1);
    }

    #[test]
    fn tokens_cap_at_capacity() {
        let now = Instant::now();
        let mut limiter = RateLimiter::new();

        // Even after a very long idle period, tokens should not exceed burst.
        let much_later = now + Duration::from_secs(3600);
        // Consume burst + 1 to verify we only get DEFAULT_BURST.
        for _ in 0..DEFAULT_BURST {
            limiter.check_at("key-cap", much_later).unwrap();
        }
        assert!(limiter.check_at("key-cap", much_later).is_err());
    }

    #[test]
    fn rejection_error_has_resource_exhausted_code() {
        let now = Instant::now();
        let mut limiter = RateLimiter::new();

        for _ in 0..DEFAULT_BURST {
            limiter.check_at("key-code", now).unwrap();
        }

        let rejection = limiter.check_at("key-code", now).unwrap_err();
        assert_eq!(rejection.error.code, RESOURCE_EXHAUSTED);
        assert!(rejection.error.trace_id.starts_with("req-"));
    }
}
