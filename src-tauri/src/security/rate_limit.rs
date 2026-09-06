use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Simple sliding-window rate limiter (in-memory).
///
/// Java comparison: like a ConcurrentHashMap<String, Bucket> with
/// synchronized access. In Rust, Mutex<HashMap<...>> achieves the same.
///
/// In production, consider using a tower middleware layer or Redis
/// for distributed rate limiting. This is sufficient for single-instance
/// local desktop use.
pub struct RateLimiter {
    /// Map: API key hash -> (request count, window start)
    buckets: Mutex<HashMap<String, (u32, Instant)>>,
    /// Requests per minute limit
    rpm: u32,
    /// Window size (60 seconds for RPM)
    window: Duration,
}

impl RateLimiter {
    pub fn new(rpm: u32) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            rpm,
            window: Duration::from_secs(60),
        }
    }

    /// Check if a request is allowed under the rate limit.
    /// Returns Ok(()) if allowed, Err(message) if rejected.
    pub fn check(&self, key: &str) -> Result<(), String> {
        let mut buckets = self.buckets.lock().map_err(|e| e.to_string())?;
        let now = Instant::now();

        let entry = buckets.entry(key.to_string()).or_insert((0, now));

        // Reset window if expired
        if now.duration_since(entry.1) >= self.window {
            *entry = (0, now);
        }

        if entry.0 >= self.rpm {
            return Err(format!(
                "Rate limit exceeded: {} requests/minute (key: {}...)",
                self.rpm,
                &key[..key.len().min(8)]
            ));
        }

        entry.0 += 1;
        Ok(())
    }

    /// Clean up expired buckets (call periodically)
    pub fn cleanup(&self) {
        if let Ok(mut buckets) = self.buckets.lock() {
            let now = Instant::now();
            buckets.retain(|_, (_, window_start)| now.duration_since(*window_start) < self.window);
        }
    }
}

/// 运行态限速器：随设置启停 / 改 RPM 时整体替换。
///
/// `RateLimiter` 自身不支持运行时改限额（`set_rpm` 为占位），故设置变更时
/// 由调用方重建（见 `commands/settings.rs` 的 `update_settings` 与 `lib.rs` 启动）。
pub struct RateLimiterState {
    /// 是否启用限流（对应设置 `enable_rate_limit`）
    pub enabled: bool,
    /// 实际限速器（按 Key 滑动窗口）
    pub limiter: RateLimiter,
}

impl RateLimiterState {
    pub fn new(enabled: bool, rpm: u32) -> Self {
        Self {
            enabled,
            limiter: RateLimiter::new(rpm),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allows_under_limit() {
        let limiter = RateLimiter::new(5);
        for _ in 0..5 {
            assert!(limiter.check("test-key").is_ok());
        }
    }

    #[test]
    fn test_rejects_over_limit() {
        let limiter = RateLimiter::new(3);
        for _ in 0..3 {
            assert!(limiter.check("test-key").is_ok());
        }
        assert!(limiter.check("test-key").is_err());
    }

    #[test]
    fn test_separate_keys() {
        let limiter = RateLimiter::new(2);
        assert!(limiter.check("key-a").is_ok());
        assert!(limiter.check("key-a").is_ok());
        assert!(limiter.check("key-b").is_ok()); // Different key, separate bucket
    }
}
