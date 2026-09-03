//! The risk gate. Every [`Order`] must pass [`RiskGate::check`] before an
//! executor is allowed to act on it. This is the single choke point that keeps
//! a misbehaving strategy (or decision model) from doing unbounded damage.

use crate::portfolio::Portfolio;
use crate::types::{Order, Side};
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

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_order_notional: dec!(100),
            max_position_fraction: dec!(0.10),
            max_daily_loss: dec!(50),
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
    #[error("cannot size order: no reference price available")]
    NoReferencePrice,
    #[error("insufficient cash: need {need}, have {have}")]
    InsufficientCash { need: Decimal, have: Decimal },
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

    /// Check `order` against the config and the current `portfolio`. `ref_price`
    /// is the best available mark for the asset (limit price, oracle, or last
    /// trade) and is required for sizing checks.
    pub fn check(
        &self,
        order: &Order,
        portfolio: &Portfolio,
        ref_price: Option<Decimal>,
        equity: Decimal,
    ) -> Result<(), RiskReject> {
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

        if portfolio.realized_pnl() <= -self.cfg.max_daily_loss {
            return Err(RiskReject::DailyLoss {
                realized: portfolio.realized_pnl(),
                limit: self.cfg.max_daily_loss,
            });
        }

        let price = order
            .limit_price
            .or(ref_price)
            .ok_or(RiskReject::NoReferencePrice)?;
        let notional = order.qty * price;

        if notional > self.cfg.max_order_notional {
            return Err(RiskReject::NotionalCap {
                got: notional,
                cap: self.cfg.max_order_notional,
            });
        }

        match order.side {
            Side::Buy => {
                if notional > portfolio.cash() {
                    return Err(RiskReject::InsufficientCash {
                        need: notional,
                        have: portfolio.cash(),
                    });
                }
                let resulting = (portfolio.position(&order.asset) + order.qty) * price;
                let frac = if equity > dec!(0) {
                    resulting / equity
                } else {
                    dec!(0)
                };
                if frac > self.cfg.max_position_fraction {
                    return Err(RiskReject::PositionCap {
                        got: frac,
                        cap: self.cfg.max_position_fraction,
                    });
                }
            }
            Side::Sell => {
                // Selling reduces exposure; only the notional + slippage caps
                // above apply. Shorting is not modelled.
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Asset, OrderId, Venue};

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
                &p,
                None,
                dec!(1000),
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
                &p,
                None,
                dec!(10_000),
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
        // 60 notional on 1000 equity = 6% ok; 200 = 20% rejected
        assert!(gate
            .check(
                &order(Side::Buy, dec!(6), dec!(10), dec!(0.01)),
                &p,
                None,
                dec!(1000)
            )
            .is_ok());
        let err = gate
            .check(
                &order(Side::Buy, dec!(20), dec!(10), dec!(0.01)),
                &p,
                None,
                dec!(1000),
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
        // force realized pnl to -60
        use crate::types::Fill;
        use chrono::Utc;
        p.apply(&Fill {
            order_id: OrderId::new("x"),
            asset: Asset::symbol("ROAR"),
            side: Side::Buy,
            qty: dec!(10),
            price: dec!(10),
            fee: dec!(0),
            venue: Venue::Paper,
            at: Utc::now(),
        });
        p.apply(&Fill {
            order_id: OrderId::new("x"),
            asset: Asset::symbol("ROAR"),
            side: Side::Sell,
            qty: dec!(10),
            price: dec!(4),
            fee: dec!(0),
            venue: Venue::Paper,
            at: Utc::now(),
        });
        let err = gate
            .check(
                &order(Side::Buy, dec!(1), dec!(10), dec!(0.01)),
                &p,
                None,
                dec!(940),
            )
            .unwrap_err();
        assert!(matches!(err, RiskReject::DailyLoss { .. }));
    }
}
