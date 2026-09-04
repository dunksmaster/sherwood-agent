//! Time, injected.
//!
//! Strategy and gate code must not read the wall clock directly — a test that
//! calls `Utc::now()` in an assertion cannot be deterministic, and backtest,
//! paper, and live must run the same code. Callers hold a [`Clock`] and pass
//! `now` in as data.

use chrono::{DateTime, Utc};

/// A source of "now".
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

/// The real clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// A clock frozen at a fixed instant, for tests and deterministic replay.
#[derive(Debug, Clone, Copy)]
pub struct FixedClock(pub DateTime<Utc>);

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn fixed_clock_does_not_move() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let c = FixedClock(t);
        assert_eq!(c.now(), t);
        assert_eq!(c.now(), c.now());
    }
}
