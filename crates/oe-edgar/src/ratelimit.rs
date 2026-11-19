//! A thread-safe limiter that spaces requests evenly.

use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct RateLimiter {
    interval: Duration,
    next_slot: Mutex<Option<Instant>>,
}

impl RateLimiter {
    pub fn per_second(n: u32) -> Self {
        RateLimiter {
            interval: Duration::from_secs(1) / n.max(1),
            next_slot: Mutex::new(None),
        }
    }

    /// Reserves the next slot and returns how long the caller must wait.
    pub fn reserve(&self, now: Instant) -> Duration {
        let mut next = self.next_slot.lock().unwrap_or_else(|e| e.into_inner());
        let slot = match *next {
            Some(t) if t > now => t,
            _ => now,
        };
        *next = Some(slot + self.interval);
        slot - now
    }

    /// Blocks until the caller may send its next request.
    pub fn acquire(&self) {
        let wait = self.reserve(Instant::now());
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spaces_reservations_evenly() {
        let limiter = RateLimiter::per_second(10);
        let now = Instant::now();
        let waits: Vec<Duration> = (0..4).map(|_| limiter.reserve(now)).collect();
        assert_eq!(waits[0], Duration::ZERO);
        assert_eq!(waits[1], Duration::from_millis(100));
        assert_eq!(waits[3], Duration::from_millis(300));
    }

    #[test]
    fn idle_time_is_not_banked() {
        let limiter = RateLimiter::per_second(10);
        let t0 = Instant::now();
        limiter.reserve(t0);
        // A long pause must not allow a burst afterwards.
        let later = t0 + Duration::from_secs(5);
        assert_eq!(limiter.reserve(later), Duration::ZERO);
        assert_eq!(limiter.reserve(later), Duration::from_millis(100));
    }

    #[test]
    fn acquire_really_waits() {
        let limiter = RateLimiter::per_second(20);
        let start = Instant::now();
        for _ in 0..5 {
            limiter.acquire();
        }
        assert!(start.elapsed() >= Duration::from_millis(195));
    }
}
