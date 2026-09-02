//! Login backoff (spec §7.1).
//!
//! Failures are counted per (client address, username). After a small number
//! of free attempts the delay doubles on each further failure, capped so a
//! locked-out administrator is never locked out forever. State is in memory:
//! it is a throttle, not an audit trail, and a restart of the manager is not
//! something an attacker can trigger.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Failures allowed before any delay applies.
const FREE_ATTEMPTS: u32 = 5;
/// Delay after the first non-free failure; doubles from there.
const BASE_DELAY: Duration = Duration::from_secs(2);
/// Ceiling on the delay.
const MAX_DELAY: Duration = Duration::from_secs(900);
/// A key with no activity for this long is forgotten.
const ENTRY_TTL: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, Copy)]
struct Entry {
    failures: u32,
    last_failure: Instant,
}

#[derive(Default)]
pub struct LoginLimiter {
    entries: Mutex<HashMap<String, Entry>>,
}

impl LoginLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seconds the caller must wait, or `None` when a login may be attempted.
    pub fn retry_after(&self, key: &str) -> Option<u64> {
        self.retry_after_at(key, Instant::now())
    }

    /// Records a failed attempt and returns the new failure count.
    pub fn record_failure(&self, key: &str) -> u32 {
        self.record_failure_at(key, Instant::now())
    }

    /// Clears the counter after a successful login.
    pub fn reset(&self, key: &str) {
        let mut entries = self.entries.lock().expect("limiter mutex poisoned");
        entries.remove(key);
    }

    fn retry_after_at(&self, key: &str, now: Instant) -> Option<u64> {
        let entries = self.entries.lock().expect("limiter mutex poisoned");
        let entry = entries.get(key)?;
        if now.duration_since(entry.last_failure) >= ENTRY_TTL {
            return None;
        }
        let delay = delay_for(entry.failures)?;
        let elapsed = now.duration_since(entry.last_failure);
        if elapsed >= delay {
            return None;
        }
        // Round up so a caller that waits exactly this many seconds is let in.
        Some((delay - elapsed).as_secs() + 1)
    }

    fn record_failure_at(&self, key: &str, now: Instant) -> u32 {
        let mut entries = self.entries.lock().expect("limiter mutex poisoned");
        entries.retain(|_, entry| now.duration_since(entry.last_failure) < ENTRY_TTL);
        let entry = entries.entry(key.to_string()).or_insert(Entry {
            failures: 0,
            last_failure: now,
        });
        entry.failures = entry.failures.saturating_add(1);
        entry.last_failure = now;
        entry.failures
    }
}

/// Backoff curve: nothing for the first [`FREE_ATTEMPTS`], then 2s, 4s, 8s …
fn delay_for(failures: u32) -> Option<Duration> {
    if failures <= FREE_ATTEMPTS {
        return None;
    }
    let steps = failures - FREE_ATTEMPTS - 1;
    let multiplier = 1u64.checked_shl(steps.min(32)).unwrap_or(u64::MAX);
    let delay = BASE_DELAY
        .checked_mul(multiplier.min(u32::MAX as u64) as u32)
        .unwrap_or(MAX_DELAY);
    Some(delay.min(MAX_DELAY))
}

/// Limiter key. The username is part of it so one account being attacked does
/// not lock every other login attempt from the same address, and the address
/// is part of it so a single attacker cannot lock the real administrator out
/// from elsewhere for long.
pub fn key(client: &str, username: &str) -> String {
    format!("{client}|{username}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_attempts_are_free() {
        let limiter = LoginLimiter::new();
        for _ in 0..FREE_ATTEMPTS {
            limiter.record_failure("k");
            assert_eq!(limiter.retry_after("k"), None);
        }
    }

    #[test]
    fn the_delay_doubles_after_the_free_attempts() {
        assert_eq!(delay_for(FREE_ATTEMPTS), None);
        assert_eq!(delay_for(FREE_ATTEMPTS + 1), Some(Duration::from_secs(2)));
        assert_eq!(delay_for(FREE_ATTEMPTS + 2), Some(Duration::from_secs(4)));
        assert_eq!(delay_for(FREE_ATTEMPTS + 3), Some(Duration::from_secs(8)));
    }

    #[test]
    fn the_delay_is_capped() {
        assert_eq!(delay_for(FREE_ATTEMPTS + 40), Some(MAX_DELAY));
        assert_eq!(delay_for(u32::MAX), Some(MAX_DELAY));
    }

    #[test]
    fn a_blocked_key_reports_a_retry_after() {
        let limiter = LoginLimiter::new();
        for _ in 0..=FREE_ATTEMPTS {
            limiter.record_failure("k");
        }
        let retry = limiter.retry_after("k").expect("should be throttled");
        assert!((1..=3).contains(&retry), "unexpected retry_after {retry}");
    }

    #[test]
    fn waiting_out_the_delay_unblocks() {
        let limiter = LoginLimiter::new();
        let start = Instant::now();
        for _ in 0..=FREE_ATTEMPTS {
            limiter.record_failure_at("k", start);
        }
        assert!(limiter.retry_after_at("k", start).is_some());
        assert!(limiter
            .retry_after_at("k", start + Duration::from_secs(3))
            .is_none());
    }

    #[test]
    fn a_successful_login_clears_the_counter() {
        let limiter = LoginLimiter::new();
        for _ in 0..=FREE_ATTEMPTS + 3 {
            limiter.record_failure("k");
        }
        assert!(limiter.retry_after("k").is_some());
        limiter.reset("k");
        assert_eq!(limiter.retry_after("k"), None);
    }

    #[test]
    fn keys_are_scoped_per_address_and_username() {
        let limiter = LoginLimiter::new();
        let attacker = key("10.0.0.9", "admin");
        let owner = key("10.0.0.1", "admin");
        for _ in 0..=FREE_ATTEMPTS + 2 {
            limiter.record_failure(&attacker);
        }
        assert!(limiter.retry_after(&attacker).is_some());
        assert_eq!(limiter.retry_after(&owner), None);
    }

    #[test]
    fn stale_entries_are_forgotten() {
        let limiter = LoginLimiter::new();
        let start = Instant::now();
        for _ in 0..=FREE_ATTEMPTS + 5 {
            limiter.record_failure_at("k", start);
        }
        assert!(limiter
            .retry_after_at("k", start + ENTRY_TTL + Duration::from_secs(1))
            .is_none());
    }
}
