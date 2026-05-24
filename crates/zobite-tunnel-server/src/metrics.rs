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
