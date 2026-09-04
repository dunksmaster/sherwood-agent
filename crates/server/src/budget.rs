//! Per-session spend budgets — hard stops that cap how much a single
//! `sherwood serve` session will let through, independent of the risk config.
//!
//! Three caps, any of which, once tripped, denies every further place-order
//! until an admin resets the session:
//!
//! * **order count** — how many orders may be allowed;
//! * **notional** — cumulative `qty * price` across allowed orders;
//! * **duration** — wall-clock time since the session (or last reset) started.
//!
//! A cap of `0` means "no limit". Reads and cancels never touch the budget.

use rust_decimal::Decimal;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct BudgetCaps {
    /// Max place-orders to allow. `0` = unlimited.
    pub max_orders: u32,
    /// Max cumulative notional. `0` (or negative) = unlimited.
    pub max_notional: Decimal,
    /// Max session wall-clock. `0` = unlimited.
    pub max_duration: Duration,
}

impl Default for BudgetCaps {
    fn default() -> Self {
        Self {
            max_orders: 0,
            max_notional: Decimal::ZERO,
            max_duration: Duration::ZERO,
        }
    }
}

impl BudgetCaps {
    pub fn is_unlimited(&self) -> bool {
        self.max_orders == 0 && self.max_notional <= Decimal::ZERO && self.max_duration.is_zero()
    }
}

/// Why an order was refused by the budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetBreach {
    OrderCount { cap: u32 },
    Notional { would_be: Decimal, cap: Decimal },
    Duration { elapsed_secs: u64, cap_secs: u64 },
}

impl std::fmt::Display for BudgetBreach {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OrderCount { cap } => write!(f, "session order limit reached ({cap})"),
            Self::Notional { would_be, cap } => {
                write!(
                    f,
                    "session notional limit reached ({would_be} would exceed {cap})"
                )
            }
            Self::Duration {
                elapsed_secs,
                cap_secs,
            } => write!(
                f,
                "session time limit reached ({elapsed_secs}s of {cap_secs}s)"
            ),
        }
    }
}

/// A point-in-time view for `GET /v1/session`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BudgetView {
    pub orders_used: u32,
    pub orders_cap: u32,
    pub notional_used: Decimal,
    pub notional_cap: Decimal,
    pub elapsed_secs: u64,
    pub duration_cap_secs: u64,
    pub breached: bool,
}

struct State {
    orders_used: u32,
    notional_used: Decimal,
    started: Instant,
    breached: bool,
}

pub struct SessionBudget {
    caps: BudgetCaps,
    state: Mutex<State>,
}

impl SessionBudget {
    pub fn new(caps: BudgetCaps) -> Self {
        Self {
            caps,
            state: Mutex::new(State {
                orders_used: 0,
                notional_used: Decimal::ZERO,
                started: Instant::now(),
                breached: false,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Check every cap for one about-to-be-allowed order of `notional`. On
    /// success the order is recorded against the running totals. On breach the
    /// session is latched breached and every later call fails until [`reset`].
    ///
    /// [`reset`]: Self::reset
    pub fn try_record(&self, notional: Decimal) -> Result<(), BudgetBreach> {
        let mut st = self.lock();

        if st.breached {
            // Report whichever cap is already blown; order count is the cheapest.
            return Err(self.first_breach(&st));
        }

        if !self.caps.max_duration.is_zero() && st.started.elapsed() >= self.caps.max_duration {
            st.breached = true;
            return Err(BudgetBreach::Duration {
                elapsed_secs: st.started.elapsed().as_secs(),
                cap_secs: self.caps.max_duration.as_secs(),
            });
        }
        if self.caps.max_orders > 0 && st.orders_used >= self.caps.max_orders {
            st.breached = true;
            return Err(BudgetBreach::OrderCount {
                cap: self.caps.max_orders,
            });
        }
        let would_be = st.notional_used + notional.max(Decimal::ZERO);
        if self.caps.max_notional > Decimal::ZERO && would_be > self.caps.max_notional {
            st.breached = true;
            return Err(BudgetBreach::Notional {
                would_be,
                cap: self.caps.max_notional,
            });
        }

        st.orders_used += 1;
        st.notional_used = would_be;
        Ok(())
    }

    fn first_breach(&self, st: &State) -> BudgetBreach {
        if self.caps.max_orders > 0 && st.orders_used >= self.caps.max_orders {
            BudgetBreach::OrderCount {
                cap: self.caps.max_orders,
            }
        } else if !self.caps.max_duration.is_zero()
            && st.started.elapsed() >= self.caps.max_duration
        {
            BudgetBreach::Duration {
                elapsed_secs: st.started.elapsed().as_secs(),
                cap_secs: self.caps.max_duration.as_secs(),
            }
        } else {
            BudgetBreach::Notional {
                would_be: st.notional_used,
                cap: self.caps.max_notional,
            }
        }
    }

    /// Zero the counters and restart the clock. Clears a breach.
    pub fn reset(&self) {
        let mut st = self.lock();
        st.orders_used = 0;
        st.notional_used = Decimal::ZERO;
        st.started = Instant::now();
        st.breached = false;
    }

    pub fn view(&self) -> BudgetView {
        let st = self.lock();
        BudgetView {
            orders_used: st.orders_used,
            orders_cap: self.caps.max_orders,
            notional_used: st.notional_used,
            notional_cap: self.caps.max_notional,
            elapsed_secs: st.started.elapsed().as_secs(),
            duration_cap_secs: self.caps.max_duration.as_secs(),
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
        let b = SessionBudget::new(BudgetCaps::default());
        for _ in 0..1000 {
            assert!(b.try_record(dec!(1_000_000)).is_ok());
        }
        assert!(!b.view().breached);
    }

    #[test]
    fn order_count_cap_latches() {
        let b = SessionBudget::new(BudgetCaps {
            max_orders: 2,
            ..BudgetCaps::default()
        });
        assert!(b.try_record(dec!(1)).is_ok());
        assert!(b.try_record(dec!(1)).is_ok());
        assert_eq!(
            b.try_record(dec!(1)).unwrap_err(),
            BudgetBreach::OrderCount { cap: 2 }
        );
        // latched — a fourth call still fails
        assert!(b.try_record(dec!(1)).is_err());
        assert!(b.view().breached);
    }

    #[test]
    fn notional_cap_blocks_the_order_that_would_exceed_it() {
        let b = SessionBudget::new(BudgetCaps {
            max_notional: dec!(100),
            ..BudgetCaps::default()
        });
        assert!(b.try_record(dec!(60)).is_ok());
        assert!(matches!(
            b.try_record(dec!(50)).unwrap_err(),
            BudgetBreach::Notional { .. }
        ));
        assert_eq!(b.view().notional_used, dec!(60)); // the rejected order was not counted
    }

    #[test]
    fn duration_cap_trips() {
        let b = SessionBudget::new(BudgetCaps {
            max_duration: Duration::from_millis(20),
            ..BudgetCaps::default()
        });
        assert!(b.try_record(dec!(1)).is_ok());
        std::thread::sleep(Duration::from_millis(35));
        assert!(matches!(
            b.try_record(dec!(1)).unwrap_err(),
            BudgetBreach::Duration { .. }
        ));
    }

    #[test]
    fn reset_clears_a_breach() {
        let b = SessionBudget::new(BudgetCaps {
            max_orders: 1,
            ..BudgetCaps::default()
        });
        assert!(b.try_record(dec!(1)).is_ok());
        assert!(b.try_record(dec!(1)).is_err());
        b.reset();
        assert!(!b.view().breached);
        assert!(b.try_record(dec!(1)).is_ok());
    }
}
