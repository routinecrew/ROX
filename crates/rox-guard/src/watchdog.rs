//! GuardWatchdog — monitors Guard process liveness.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Watchdog that monitors Guard process heartbeats.
pub struct GuardWatchdog {
    heartbeat_interval: Duration,
    timeout: Duration,
    last_heartbeat: Instant,
    heartbeat_count: Arc<AtomicU64>,
    missed_count: u64,
}

impl GuardWatchdog {
    /// Create a new watchdog.
    pub fn new(heartbeat_interval_ms: u64, timeout_ms: u64) -> Self {
        Self {
            heartbeat_interval: Duration::from_millis(heartbeat_interval_ms),
            timeout: Duration::from_millis(timeout_ms),
            last_heartbeat: Instant::now(),
            heartbeat_count: Arc::new(AtomicU64::new(0)),
            missed_count: 0,
        }
    }

    /// Record a heartbeat from the Guard process.
    pub fn heartbeat(&mut self) {
        self.last_heartbeat = Instant::now();
        self.heartbeat_count.fetch_add(1, Ordering::Relaxed);
        self.missed_count = 0;
    }

    /// Check if the Guard is still alive.
    pub fn is_alive(&self) -> bool {
        self.last_heartbeat.elapsed() < self.timeout
    }

    /// Check and return true if timeout has occurred.
    pub fn check_timeout(&mut self) -> bool {
        if self.last_heartbeat.elapsed() >= self.timeout {
            self.missed_count += 1;
            true
        } else {
            false
        }
    }

    /// Duration since last heartbeat.
    pub fn time_since_heartbeat(&self) -> Duration {
        self.last_heartbeat.elapsed()
    }

    /// Heartbeat interval.
    pub fn interval(&self) -> Duration {
        self.heartbeat_interval
    }

    /// Total heartbeats received.
    pub fn total_heartbeats(&self) -> u64 {
        self.heartbeat_count.load(Ordering::Relaxed)
    }

    /// Number of consecutive missed heartbeats.
    pub fn missed_count(&self) -> u64 {
        self.missed_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watchdog_alive() {
        let mut wd = GuardWatchdog::new(100, 500);
        wd.heartbeat();
        assert!(wd.is_alive());
        assert!(!wd.check_timeout());
    }

    #[test]
    fn test_watchdog_timeout() {
        let mut wd = GuardWatchdog::new(100, 10); // 10ms timeout
                                                  // Force timeout by using an old last_heartbeat
        wd.last_heartbeat = Instant::now() - Duration::from_millis(20);
        assert!(!wd.is_alive());
        assert!(wd.check_timeout());
    }
}
