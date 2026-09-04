//! The risk gate. Every [`Order`] must pass [`RiskGate::check`] before an
//! executor is allowed to act on it. This is the single choke point that keeps
//! a misbehaving strategy (or decision model) from doing unbounded damage.
//!
//! Two classes of check:
//!
//! * **Hard stops** — kill switch and the realized daily-loss breaker reject
//!   *every* order, buys and sells alike. They mean "stop trading".
//! * **Entry limits** — notional, position fraction, unrealized loss, open
//!   position count, and per-symbol cooldown gate *new exposure* only. A sell
//!   that reduces exposure is never blocked by these; you can always de-risk.

use crate::portfolio::Portfolio;
use crate::types::{Order, Side};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    /// Hard cap on notional (qty * price) for a single order, in the cash asset.
    pub max_order_notional: Decimal,
    /// Cap on a single asset's position as a fraction of equity.
    pub max_position_fraction: Decimal,
    /// If realized P&L for the session drops below `-max_daily_loss`, every
    /// order is rejected until the operator resets.
    pub max_daily_loss: Decimal,
    /// If mark-to-market unrealized P&L across open positions drops below
    /// `-max_unrealized_loss`, new buys are rejected (sells still allowed).
    #[serde(default = "default_max_unrealized_loss")]
    pub max_unrealized_loss: Decimal,
    /// Reject a buy that would open a *new* symbol when this many symbols are
    /// already held. Adding to an existing position is unaffected.
    #[serde(default = "default_max_open_positions")]
    pub max_open_positions: usize,
    /// Minimum seconds between buys for the same symbol. `0` disables it.
    #[serde(default)]
    pub order_cooldown_secs: u64,
    /// Reject any order whose `max_slippage` exceeds this.
    pub max_slippage: Decimal,
    /// If non-empty, only these symbols may be traded.
    #[serde(default)]
    pub allowlist: HashSet<String>,
    /// These symbols may never be traded (takes precedence over the allowlist).
    #[serde(default)]
    pub denylist: HashSet<String>,
    /// Global stop. When true, all orders are rejected.
    #[serde(default)]
    pub kill_switch: bool,
}

fn default_max_unrealized_loss() -> Decimal {
    dec!(100)
}
fn default_max_open_positions() -> usize {
    10
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_order_notional: dec!(100),
            max_position_fraction: dec!(0.10),
            max_daily_loss: dec!(50),
            max_unrealized_loss: default_max_unrealized_loss(),
            max_open_positions: default_max_open_positions(),
            order_cooldown_secs: 0,
            max_slippage: dec!(0.02),
            allowlist: HashSet::new(),
            denylist: HashSet::new(),
            kill_switch: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RiskReject {
    #[error("kill switch is engaged")]
    KillSwitch,
    #[error("{0} is on the denylist")]
    Denylisted(String),
    #[error("{0} is not on the allowlist")]
    NotAllowlisted(String),
    #[error("order notional {got} exceeds cap {cap}")]
    NotionalCap { got: Decimal, cap: Decimal },
    #[error("resulting position fraction {got} exceeds cap {cap}")]
    PositionCap { got: Decimal, cap: Decimal },
    #[error("order slippage {got} exceeds cap {cap}")]
    SlippageCap { got: Decimal, cap: Decimal },
    #[error("daily loss limit hit (realized {realized}, limit {limit})")]
    DailyLoss { realized: Decimal, limit: Decimal },
    #[error("unrealized loss limit hit ({unrealized}, limit {limit})")]
    UnrealizedLoss { unrealized: Decimal, limit: Decimal },
    #[error("already holding {open} positions (limit {limit})")]
    TooManyOpenPositions { open: usize, limit: usize },
    #[error("{symbol} is in cooldown for another {remaining_secs}s")]
    Cooldown { symbol: String, remaining_secs: i64 },
    #[error("cannot size order: no reference price available")]
    NoReferencePrice,
    #[error("insufficient cash: need {need}, have {have}")]
    InsufficientCash { need: Decimal, have: Decimal },
}

/// Everything the gate needs about the world beyond its own config. The caller
/// builds this from the portfolio and a price oracle so the gate stays pure and
/// synchronous.
pub struct GateContext<'a> {
    pub portfolio: &'a Portfolio,
    /// Best available mark for the order's asset (limit price, oracle, last trade).
    pub ref_price: Option<Decimal>,
    /// Mark-to-market equity.
    pub equity: Decimal,
    /// Mark-to-market unrealized P&L across all open positions (negative = loss).
    pub unrealized_pnl: Decimal,
    /// When the last *accepted* order for this symbol was placed, if any.
    pub last_order_at: Option<DateTime<Utc>>,
    /// Now — injected, never read from the wall clock inside the gate.
    pub now: DateTime<Utc>,
}

pub struct RiskGate {
    cfg: RiskConfig,
}

impl RiskGate {
    pub fn new(cfg: RiskConfig) -> Self {
        Self { cfg }
    }

    pub fn config(&self) -> &RiskConfig {
        &self.cfg
    }

    pub fn config_mut(&mut self) -> &mut RiskConfig {
        &mut self.cfg
    }

    /// Check `order` against the config and `ctx`.
    pub fn check(&self, order: &Order, ctx: &GateContext<'_>) -> Result<(), RiskReject> {
        // ---- hard stops: reject everything ----
        if self.cfg.kill_switch {
            return Err(RiskReject::KillSwitch);
        }

        let sym = &order.asset.symbol;
        if self.cfg.denylist.contains(sym) {
            return Err(RiskReject::Denylisted(sym.clone()));
        }
        if !self.cfg.allowlist.is_empty() && !self.cfg.allowlist.contains(sym) {
            return Err(RiskReject::NotAllowlisted(sym.clone()));
        }

        if order.max_slippage > self.cfg.max_slippage {
            return Err(RiskReject::SlippageCap {
                got: order.max_slippage,
                cap: self.cfg.max_slippage,
            });
        }

        if ctx.portfolio.realized_pnl() <= -self.cfg.max_daily_loss {
            return Err(RiskReject::DailyLoss {
                realized: ctx.portfolio.realized_pnl(),
                limit: self.cfg.max_daily_loss,
            });
        }

        let price = order
            .limit_price
            .or(ctx.ref_price)
            .ok_or(RiskReject::NoReferencePrice)?;
        let notional = order.qty * price;

        if notional > self.cfg.max_order_notional {
            return Err(RiskReject::NotionalCap {
                got: notional,
                cap: self.cfg.max_order_notional,
            });
        }

        // ---- entry limits: buys only. Sells always pass from here. ----
        if order.side == Side::Sell {
            return Ok(());
        }

        if notional > ctx.portfolio.cash() {
            return Err(RiskReject::InsufficientCash {
                need: notional,
                have: ctx.portfolio.cash(),
            });
        }

        if ctx.unrealized_pnl <= -self.cfg.max_unrealized_loss {
            return Err(RiskReject::UnrealizedLoss {
                unrealized: ctx.unrealized_pnl,
                limit: self.cfg.max_unrealized_loss,
            });
        }

        let currently_held = ctx.portfolio.position(&order.asset) != dec!(0);
        if !currently_held {
            let open = ctx.portfolio.open_position_count();
            if open >= self.cfg.max_open_positions {
                return Err(RiskReject::TooManyOpenPositions {
                    open,
                    limit: self.cfg.max_open_positions,
                });
            }
        }

        if self.cfg.order_cooldown_secs > 0 {
            if let Some(last) = ctx.last_order_at {
                let elapsed = (ctx.now - last).num_seconds();
                let cooldown = self.cfg.order_cooldown_secs as i64;
                if elapsed < cooldown {
                    return Err(RiskReject::Cooldown {
                        symbol: sym.clone(),
                        remaining_secs: cooldown - elapsed,
                    });
                }
            }
        }

        let resulting = (ctx.portfolio.position(&order.asset) + order.qty) * price;
        let frac = if ctx.equity > dec!(0) {
            resulting / ctx.equity
        } else {
            dec!(0)
        };
        if frac > self.cfg.max_position_fraction {
            return Err(RiskReject::PositionCap {
                got: frac,
                cap: self.cfg.max_position_fraction,
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Asset, Fill, OrderId, Venue};

    fn order(side: Side, qty: Decimal, price: Decimal, slip: Decimal) -> Order {
        Order {
            id: OrderId::new("t"),
            asset: Asset::symbol("ROAR"),
            side,
            qty,
            limit_price: Some(price),
            max_slippage: slip,
            venue: Venue::Paper,
            reason: "test".into(),
        }
    }

    /// A `GateContext` over `p` with sensible defaults for the fields a given
    /// test does not care about.
    fn ctx<'a>(p: &'a Portfolio, equity: Decimal) -> GateContext<'a> {
        GateContext {
            portfolio: p,
            ref_price: None,
            equity,
            unrealized_pnl: dec!(0),
            last_order_at: None,
            now: DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap(),
        }
    }

    fn buy_fill(sym: &str, qty: Decimal, price: Decimal) -> Fill {
        Fill {
            order_id: OrderId::new("x"),
            asset: Asset::symbol(sym),
            side: Side::Buy,
            qty,
            price,
            fee: dec!(0),
            venue: Venue::Paper,
            at: Utc::now(),
        }
    }

    #[test]
    fn rejects_when_kill_switch_on() {
        let gate = RiskGate::new(RiskConfig {
            kill_switch: true,
            ..Default::default()
        });
        let p = Portfolio::new(dec!(1000));
        let err = gate
            .check(
                &order(Side::Buy, dec!(1), dec!(10), dec!(0.01)),
                &ctx(&p, dec!(1000)),
            )
            .unwrap_err();
        assert_eq!(err, RiskReject::KillSwitch);
    }

    #[test]
    fn enforces_notional_cap() {
        let gate = RiskGate::new(RiskConfig {
            max_order_notional: dec!(100),
            ..Default::default()
        });
        let p = Portfolio::new(dec!(10_000));
        let err = gate
            .check(
                &order(Side::Buy, dec!(20), dec!(10), dec!(0.01)),
                &ctx(&p, dec!(10_000)),
            )
            .unwrap_err();
        assert!(matches!(err, RiskReject::NotionalCap { .. }));
    }

    #[test]
    fn enforces_position_fraction() {
        let gate = RiskGate::new(RiskConfig {
            max_order_notional: dec!(10_000),
            max_position_fraction: dec!(0.10),
            ..Default::default()
        });
        let p = Portfolio::new(dec!(1000));
        assert!(gate
            .check(
                &order(Side::Buy, dec!(6), dec!(10), dec!(0.01)),
                &ctx(&p, dec!(1000))
            )
            .is_ok());
        let err = gate
            .check(
                &order(Side::Buy, dec!(20), dec!(10), dec!(0.01)),
                &ctx(&p, dec!(1000)),
            )
            .unwrap_err();
        assert!(matches!(err, RiskReject::PositionCap { .. }));
    }

    #[test]
    fn blocks_after_daily_loss_limit() {
        let gate = RiskGate::new(RiskConfig {
            max_daily_loss: dec!(50),
            ..Default::default()
        });
        let mut p = Portfolio::new(dec!(1000));
        p.apply(&buy_fill("ROAR", dec!(10), dec!(10)));
        p.apply(&Fill {
            side: Side::Sell,
            ..buy_fill("ROAR", dec!(10), dec!(4))
        });
        let err = gate
            .check(
                &order(Side::Buy, dec!(1), dec!(10), dec!(0.01)),
                &ctx(&p, dec!(940)),
            )
            .unwrap_err();
        assert!(matches!(err, RiskReject::DailyLoss { .. }));
    }

    #[test]
    fn blocks_new_buys_past_the_unrealized_loss_limit() {
        let gate = RiskGate::new(RiskConfig {
            max_unrealized_loss: dec!(50),
            max_order_notional: dec!(10_000),
            ..Default::default()
        });
        let mut p = Portfolio::new(dec!(1000));
        p.apply(&buy_fill("ROAR", dec!(10), dec!(10)));
        let mut c = ctx(&p, dec!(940));
        c.unrealized_pnl = dec!(-60);

        assert!(matches!(
            gate.check(&order(Side::Buy, dec!(1), dec!(10), dec!(0.01)), &c)
                .unwrap_err(),
            RiskReject::UnrealizedLoss { .. }
        ));
        // a sell to de-risk is still allowed
        assert!(gate
            .check(&order(Side::Sell, dec!(1), dec!(10), dec!(0.01)), &c)
            .is_ok());
    }

    #[test]
    fn caps_the_number_of_open_positions() {
        let gate = RiskGate::new(RiskConfig {
            max_open_positions: 1,
            max_order_notional: dec!(10_000),
            max_position_fraction: dec!(1),
            ..Default::default()
        });
        let mut p = Portfolio::new(dec!(10_000));
        p.apply(&buy_fill("AAA", dec!(1), dec!(1)));

        // adding to the held symbol is fine
        let mut add = order(Side::Buy, dec!(1), dec!(1), dec!(0.01));
        add.asset = Asset::symbol("AAA");
        assert!(gate.check(&add, &ctx(&p, dec!(10_000))).is_ok());

        // opening a second symbol is refused
        let mut new_sym = order(Side::Buy, dec!(1), dec!(1), dec!(0.01));
        new_sym.asset = Asset::symbol("BBB");
        assert!(matches!(
            gate.check(&new_sym, &ctx(&p, dec!(10_000))).unwrap_err(),
            RiskReject::TooManyOpenPositions { open: 1, limit: 1 }
        ));
    }

    #[test]
    fn enforces_per_symbol_cooldown_on_buys_only() {
        let gate = RiskGate::new(RiskConfig {
            order_cooldown_secs: 60,
            max_order_notional: dec!(10_000),
            max_position_fraction: dec!(1),
            ..Default::default()
        });
        let p = Portfolio::new(dec!(10_000));
        let now = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();

        let mut c = ctx(&p, dec!(10_000));
        c.now = now;
        c.last_order_at = Some(now - chrono::Duration::seconds(30)); // 30s ago, cooldown 60

        let err = gate
            .check(&order(Side::Buy, dec!(1), dec!(1), dec!(0.01)), &c)
            .unwrap_err();
        assert_eq!(
            err,
            RiskReject::Cooldown {
                symbol: "ROAR".into(),
                remaining_secs: 30
            }
        );

        // the same-symbol sell is not gated by cooldown
        assert!(gate
            .check(&order(Side::Sell, dec!(1), dec!(1), dec!(0.01)), &c)
            .is_ok());

        // past the cooldown, the buy passes
        c.last_order_at = Some(now - chrono::Duration::seconds(90));
        assert!(gate
            .check(&order(Side::Buy, dec!(1), dec!(1), dec!(0.01)), &c)
            .is_ok());
    }

    #[test]
    fn default_config_admits_a_normal_first_buy() {
        let gate = RiskGate::new(RiskConfig {
            max_order_notional: dec!(10_000),
            max_position_fraction: dec!(1),
            ..Default::default()
        });
        let p = Portfolio::new(dec!(1000));
        assert!(gate
            .check(
                &order(Side::Buy, dec!(1), dec!(10), dec!(0.01)),
                &ctx(&p, dec!(1000))
            )
            .is_ok());
    }
}
