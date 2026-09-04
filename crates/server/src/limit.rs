//! A global fixed-window rate limiter.
//!
//! The server is loopback-only and single-operator, so per-IP buckets buy
//! nothing — every caller is `127.0.0.1`. This just stops a runaway script (or
//! a wedged agent retry loop) from hammering the process. `max == 0` disables
//! it.

use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct RateLimiter {
    max: u32,
    window: Duration,
    window_start: Mutex<(Instant, u32)>,
}

impl RateLimiter {
    /// `max` requests per `window`. `max == 0` disables limiting.
    pub fn new(max: u32, window: Duration) -> Self {
        Self {
            max,
            window,
            window_start: Mutex::new((Instant::now(), 0)),
        }
    }

    pub fn per_minute(max: u32) -> Self {
        Self::new(max, Duration::from_secs(60))
    }

    /// `true` if the request is within budget. Advances the counter.
    pub fn check(&self) -> bool {
        if self.max == 0 {
            return true;
        }
        let mut guard = self.window_start.lock().unwrap_or_else(|e| e.into_inner());
        let (start, count) = &mut *guard;
        if start.elapsed() >= self.window {
            *start = Instant::now();
            *count = 0;
        }
        if *count >= self.max {
            return false;
        }
        *count += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_max_then_blocks_within_the_window() {
        let rl = RateLimiter::new(3, Duration::from_secs(60));
        assert!(rl.check());
        assert!(rl.check());
        assert!(rl.check());
        assert!(!rl.check());
        assert!(!rl.check());
    }

    #[test]
    fn resets_after_the_window() {
        let rl = RateLimiter::new(1, Duration::from_millis(20));
        assert!(rl.check());
        assert!(!rl.check());
        std::thread::sleep(Duration::from_millis(30));
        assert!(rl.check());
    }

    #[test]
    fn zero_max_disables_the_limit() {
        let rl = RateLimiter::per_minute(0);
        for _ in 0..1000 {
            assert!(rl.check());
        }
    }
}
