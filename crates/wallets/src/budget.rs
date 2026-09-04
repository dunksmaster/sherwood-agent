//! A per-wallet spend ceiling — the same hard-stop-and-latch shape as
//! `sherwood-server`'s `SessionBudget`, scoped to one wallet instead of one
//! server session: transaction count, cumulative notional, and wall-clock
//! duration, any of which, once tripped, denies every further spend from
//! this wallet until [`WalletBudget::reset`].
//!
//! A cap of `0` (or a non-positive notional) means "no limit".

use rust_decimal::Decimal;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct WalletLimits {
    pub max_tx_count: u32,
    pub max_notional: Decimal,
    pub max_duration: Duration,
}

impl Default for WalletLimits {
    fn default() -> Self {
        Self {
            max_tx_count: 0,
            max_notional: Decimal::ZERO,
            max_duration: Duration::ZERO,
        }
    }
}

/// Why a spend was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalletBreach {
    TxCount { cap: u32 },
    Notional { would_be: Decimal, cap: Decimal },
    Duration { elapsed_secs: u64, cap_secs: u64 },
}

impl std::fmt::Display for WalletBreach {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TxCount { cap } => write!(f, "wallet transaction limit reached ({cap})"),
            Self::Notional { would_be, cap } => {
                write!(
                    f,
                    "wallet notional limit reached ({would_be} would exceed {cap})"
                )
            }
            Self::Duration {
                elapsed_secs,
                cap_secs,
            } => {
                write!(
                    f,
                    "wallet time limit reached ({elapsed_secs}s of {cap_secs}s)"
                )
            }
        }
    }
}

/// A point-in-time view of one wallet's budget.
#[derive(Debug, Clone)]
pub struct WalletBudgetView {
    pub tx_used: u32,
    pub tx_cap: u32,
    pub notional_used: Decimal,
    pub notional_cap: Decimal,
    pub elapsed_secs: u64,
    pub duration_cap_secs: u64,
    pub breached: bool,
}

struct State {
    tx_used: u32,
    notional_used: Decimal,
    started: Instant,
    breached: bool,
}

pub struct WalletBudget {
    limits: WalletLimits,
    state: Mutex<State>,
}

impl WalletBudget {
    #[must_use]
    pub fn new(limits: WalletLimits) -> Self {
        Self {
            limits,
            state: Mutex::new(State {
                tx_used: 0,
                notional_used: Decimal::ZERO,
                started: Instant::now(),
                breached: false,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Check every cap for a spend of `notional` from this wallet. On success
    /// the spend is recorded against the running totals. On breach the
    /// wallet is latched breached and every later call fails until [`reset`].
    ///
    /// [`reset`]: Self::reset
    pub fn try_reserve(&self, notional: Decimal) -> Result<(), WalletBreach> {
        let mut st = self.lock();

        if st.breached {
            return Err(self.first_breach(&st));
        }

        if !self.limits.max_duration.is_zero() && st.started.elapsed() >= self.limits.max_duration {
            st.breached = true;
            return Err(WalletBreach::Duration {
                elapsed_secs: st.started.elapsed().as_secs(),
                cap_secs: self.limits.max_duration.as_secs(),
            });
        }
        if self.limits.max_tx_count > 0 && st.tx_used >= self.limits.max_tx_count {
            st.breached = true;
            return Err(WalletBreach::TxCount {
                cap: self.limits.max_tx_count,
            });
        }
        let would_be = st.notional_used + notional.max(Decimal::ZERO);
        if self.limits.max_notional > Decimal::ZERO && would_be > self.limits.max_notional {
            st.breached = true;
            return Err(WalletBreach::Notional {
                would_be,
                cap: self.limits.max_notional,
            });
        }

        st.tx_used += 1;
        st.notional_used = would_be;
        Ok(())
    }

    fn first_breach(&self, st: &State) -> WalletBreach {
        if self.limits.max_tx_count > 0 && st.tx_used >= self.limits.max_tx_count {
            WalletBreach::TxCount {
                cap: self.limits.max_tx_count,
            }
        } else if !self.limits.max_duration.is_zero()
            && st.started.elapsed() >= self.limits.max_duration
        {
            WalletBreach::Duration {
                elapsed_secs: st.started.elapsed().as_secs(),
                cap_secs: self.limits.max_duration.as_secs(),
            }
        } else {
            WalletBreach::Notional {
                would_be: st.notional_used,
                cap: self.limits.max_notional,
            }
        }
    }

    /// Zero the counters and restart the clock. Clears a breach.
    pub fn reset(&self) {
        let mut st = self.lock();
        st.tx_used = 0;
        st.notional_used = Decimal::ZERO;
        st.started = Instant::now();
        st.breached = false;
    }

    #[must_use]
    pub fn view(&self) -> WalletBudgetView {
        let st = self.lock();
        WalletBudgetView {
            tx_used: st.tx_used,
            tx_cap: self.limits.max_tx_count,
            notional_used: st.notional_used,
            notional_cap: self.limits.max_notional,
            elapsed_secs: st.started.elapsed().as_secs(),
            duration_cap_secs: self.limits.max_duration.as_secs(),
            breached: st.breached,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn unlimited_by_default() {
        let b = WalletBudget::new(WalletLimits::default());
        for _ in 0..1000 {
            assert!(b.try_reserve(dec!(1_000_000)).is_ok());
        }
        assert!(!b.view().breached);
    }

    #[test]
    fn tx_count_cap_latches() {
        let b = WalletBudget::new(WalletLimits {
            max_tx_count: 2,
            ..WalletLimits::default()
        });
        assert!(b.try_reserve(dec!(1)).is_ok());
        assert!(b.try_reserve(dec!(1)).is_ok());
        assert_eq!(
            b.try_reserve(dec!(1)).unwrap_err(),
            WalletBreach::TxCount { cap: 2 }
        );
        assert!(b.try_reserve(dec!(1)).is_err()); // latched
        assert!(b.view().breached);
    }

    #[test]
    fn notional_cap_blocks_the_spend_that_would_exceed_it() {
        let b = WalletBudget::new(WalletLimits {
            max_notional: dec!(100),
            ..WalletLimits::default()
        });
        assert!(b.try_reserve(dec!(60)).is_ok());
        assert!(matches!(
            b.try_reserve(dec!(50)).unwrap_err(),
            WalletBreach::Notional { .. }
        ));
        assert_eq!(b.view().notional_used, dec!(60)); // the rejected spend was not counted
    }

    #[test]
    fn duration_cap_trips() {
        let b = WalletBudget::new(WalletLimits {
            max_duration: Duration::from_millis(20),
            ..WalletLimits::default()
        });
        assert!(b.try_reserve(dec!(1)).is_ok());
        std::thread::sleep(Duration::from_millis(35));
        assert!(matches!(
            b.try_reserve(dec!(1)).unwrap_err(),
            WalletBreach::Duration { .. }
        ));
    }

    #[test]
    fn reset_clears_a_breach() {
        let b = WalletBudget::new(WalletLimits {
            max_tx_count: 1,
            ..WalletLimits::default()
        });
        assert!(b.try_reserve(dec!(1)).is_ok());
        assert!(b.try_reserve(dec!(1)).is_err());
        b.reset();
        assert!(!b.view().breached);
        assert!(b.try_reserve(dec!(1)).is_ok());
    }
}
