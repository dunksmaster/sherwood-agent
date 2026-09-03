//! Order execution.
//!
//! [`Executor`] is the seam between sherwood-agent and the outside world.
//! Two implementations ship:
//!
//! * [`PaperExecutor`] — simulates fills against a price feed. This is the
//!   default and the only one wired up in the CLI.
//! * [`LiveExecutor`] — a **stub**. It intentionally does nothing except return
//!   [`ExecError::LiveNotConfigured`]. Connecting a real brokerage or on-chain
//!   router (for example the Robinhood Agentic Trading MCP) is left to the
//!   operator: you implement the adapter, you accept the agreements, you hold
//!   the keys. See `docs/LIVE_EXECUTION.md`.

use async_trait::async_trait;
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sherwood_core::{Fill, Order, Side, Venue};
use std::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("no price available for {0}")]
    NoPrice(String),
    #[error("simulated slippage {got} exceeded order tolerance {tol}")]
    SlippageExceeded { got: Decimal, tol: Decimal },
    #[error(
        "live execution is not configured. sherwood-agent ships without a live \
         adapter on purpose. Implement `Executor` against your venue and pass \
         it to the runner yourself. See docs/LIVE_EXECUTION.md"
    )]
    LiveNotConfigured,
    #[error("venue rejected order: {0}")]
    VenueRejected(String),
}

/// Anything that can turn an [`Order`] into a [`Fill`].
#[async_trait]
pub trait Executor: Send + Sync {
    async fn execute(&self, order: &Order) -> Result<Fill, ExecError>;

    /// Human-readable name for logs.
    fn name(&self) -> &'static str;
}

/// Deterministic fill simulator.
///
/// The caller supplies a price via [`PaperExecutor::set_price`]. Buys fill
/// `spread_bps` above that price, sells `spread_bps` below, and a flat
/// `fee_bps` is charged on notional. If the simulated execution price moves
/// more than the order's `max_slippage` away from its limit, the fill is
/// rejected — same contract a real venue would enforce.
pub struct PaperExecutor {
    inner: Mutex<PaperState>,
    spread_bps: Decimal,
    fee_bps: Decimal,
}

struct PaperState {
    prices: std::collections::HashMap<String, Decimal>,
    seq: u64,
}

impl PaperExecutor {
    pub fn new(spread_bps: Decimal, fee_bps: Decimal) -> Self {
        Self {
            inner: Mutex::new(PaperState {
                prices: Default::default(),
                seq: 0,
            }),
            spread_bps,
            fee_bps,
        }
    }

    pub fn set_price(&self, symbol: impl Into<String>, price: Decimal) {
        self.inner
            .lock()
            .unwrap()
            .prices
            .insert(symbol.into(), price);
    }
}

impl Default for PaperExecutor {
    fn default() -> Self {
        // 10 bps spread, 5 bps fee.
        Self::new(dec!(0.0010), dec!(0.0005))
    }
}

#[async_trait]
impl Executor for PaperExecutor {
    async fn execute(&self, order: &Order) -> Result<Fill, ExecError> {
        let mut st = self.inner.lock().unwrap();
        let mid = *st
            .prices
            .get(&order.asset.symbol)
            .ok_or_else(|| ExecError::NoPrice(order.asset.symbol.clone()))?;

        let exec_price = match order.side {
            Side::Buy => mid * (dec!(1) + self.spread_bps),
            Side::Sell => mid * (dec!(1) - self.spread_bps),
        };

        if let Some(limit) = order.limit_price {
            let slip = ((exec_price - limit) / limit).abs();
            if slip > order.max_slippage {
                return Err(ExecError::SlippageExceeded {
                    got: slip,
                    tol: order.max_slippage,
                });
            }
        }

        st.seq += 1;
        let fee = (order.qty * exec_price) * self.fee_bps;

        Ok(Fill {
            order_id: order.id.clone(),
            asset: order.asset.clone(),
            side: order.side,
            qty: order.qty,
            price: exec_price,
            fee,
            venue: Venue::Paper,
            at: Utc::now(),
        })
    }

    fn name(&self) -> &'static str {
        "paper"
    }
}

/// Placeholder for a real venue. Never fills.
///
/// Replace this with your own type that implements [`Executor`] against the
/// Robinhood Agentic Trading MCP or an on-chain router. sherwood-agent will
/// not do that wiring for you.
pub struct LiveExecutor {
    _private: (),
}

impl LiveExecutor {
    /// Constructing one is allowed; using it is not.
    pub fn unconfigured() -> Self {
        Self { _private: () }
    }
}

#[async_trait]
impl Executor for LiveExecutor {
    async fn execute(&self, _order: &Order) -> Result<Fill, ExecError> {
        Err(ExecError::LiveNotConfigured)
    }

    fn name(&self) -> &'static str {
        "live(unconfigured)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sherwood_core::{Asset, OrderId};

    fn order(side: Side, qty: Decimal, limit: Option<Decimal>, slip: Decimal) -> Order {
        Order {
            id: OrderId::new("t"),
            asset: Asset::symbol("ROAR"),
            side,
            qty,
            limit_price: limit,
            max_slippage: slip,
            venue: Venue::Paper,
            reason: "test".into(),
        }
    }

    #[tokio::test]
    async fn paper_fills_buy_above_mid_with_fee() {
        let ex = PaperExecutor::new(dec!(0.0010), dec!(0.0005));
        ex.set_price("ROAR", dec!(100));
        let fill = ex
            .execute(&order(Side::Buy, dec!(2), None, dec!(0.05)))
            .await
            .unwrap();
        assert_eq!(fill.price, dec!(100.10));
        assert_eq!(fill.fee, dec!(0.100100)); // 2 * 100.10 * 0.0005
    }

    #[tokio::test]
    async fn paper_rejects_when_slippage_exceeds_tolerance() {
        let ex = PaperExecutor::new(dec!(0.02), dec!(0));
        ex.set_price("ROAR", dec!(100));
        let err = ex
            .execute(&order(Side::Buy, dec!(1), Some(dec!(100)), dec!(0.01)))
            .await
            .unwrap_err();
        assert!(matches!(err, ExecError::SlippageExceeded { .. }));
    }

    #[tokio::test]
    async fn live_executor_never_fills() {
        let ex = LiveExecutor::unconfigured();
        let err = ex
            .execute(&order(Side::Buy, dec!(1), None, dec!(0.05)))
            .await
            .unwrap_err();
        assert!(matches!(err, ExecError::LiveNotConfigured));
    }
}
