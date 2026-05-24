//! Global server metrics.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Server-wide metrics.
pub struct Metrics {
    pub started_at: Instant,
    pub total_requests: AtomicU64,
    pub total_bytes_in: AtomicU64,
    pub total_bytes_out: AtomicU64,
    pub active_connections: AtomicU64,
    pub total_connections: AtomicU64,
    pub failed_auth: AtomicU64,
    pub rate_limited: AtomicU64,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            started_at: Instant::now(),
            total_requests: AtomicU64::new(0),
            total_bytes_in: AtomicU64::new(0),
            total_bytes_out: AtomicU64::new(0),
            active_connections: AtomicU64::new(0),
            total_connections: AtomicU64::new(0),
            failed_auth: AtomicU64::new(0),
            rate_limited: AtomicU64::new(0),
        }
    }
}

/// Serializable metrics snapshot for the dashboard.
#[derive(serde::Serialize)]
pub struct MetricsSnapshot {
    pub uptime_secs: u64,
    pub total_requests: u64,
    pub total_bytes_in: u64,
    pub total_bytes_out: u64,
    pub active_connections: u64,
    pub total_connections: u64,
    pub failed_auth: u64,
    pub rate_limited: u64,
}

impl Metrics {
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            uptime_secs: self.started_at.elapsed().as_secs(),
            total_requests: self.total_requests.load(Ordering::Relaxed),
            total_bytes_in: self.total_bytes_in.load(Ordering::Relaxed),
            total_bytes_out: self.total_bytes_out.load(Ordering::Relaxed),
            active_connections: self.active_connections.load(Ordering::Relaxed),
            total_connections: self.total_connections.load(Ordering::Relaxed),
            failed_auth: self.failed_auth.load(Ordering::Relaxed),
            rate_limited: self.rate_limited.load(Ordering::Relaxed),
        }
    }
}

/// Simple per-key sliding-window rate limiter.
pub struct RateLimiter {
    max_per_second: u32,
    counters: dashmap::DashMap<String, (Instant, u32)>,
}

impl RateLimiter {
    pub fn new(max_per_second: u32) -> Self {
        Self {
            max_per_second,
            counters: dashmap::DashMap::new(),
        }
    }

    /// Returns true if the request is allowed.
    pub fn check(&self, key: &str) -> bool {
        if self.max_per_second == 0 {
            return true;
        }
        let now = Instant::now();
        let mut entry = self.counters.entry(key.to_string()).or_insert((now, 0));
        let (window_start, count) = entry.value_mut();

        if now.duration_since(*window_start).as_secs() >= 1 {
            *window_start = now;
            *count = 1;
            true
        } else if *count < self.max_per_second {
            *count += 1;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter_disabled() {
        let rl = RateLimiter::new(0);
        // When max_per_second is 0, all requests are allowed
        for _ in 0..1000 {
            assert!(rl.check("test"));
        }
    }

    #[test]
    fn test_rate_limiter_basic() {
        let rl = RateLimiter::new(3);
        assert!(rl.check("client-a")); // 1
        assert!(rl.check("client-a")); // 2
        assert!(rl.check("client-a")); // 3
        assert!(!rl.check("client-a")); // 4 → rejected

        // Different key should have its own counter
        assert!(rl.check("client-b"));
    }

    #[test]
    fn test_metrics_snapshot() {
        let m = Metrics::new();
        m.total_requests.fetch_add(42, Ordering::Relaxed);
        m.failed_auth.fetch_add(3, Ordering::Relaxed);

        let snap = m.snapshot();
        assert_eq!(snap.total_requests, 42);
        assert_eq!(snap.failed_auth, 3);
        assert_eq!(snap.active_connections, 0);
        assert!(snap.uptime_secs < 2); // just created
    }
}
