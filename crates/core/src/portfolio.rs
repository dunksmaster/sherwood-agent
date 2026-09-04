//! An in-memory portfolio ledger.
//!
//! The ledger holds no I/O itself. `sherwood-store` persists it by serialising
//! the whole struct to a snapshot row and replaying nothing — the snapshot *is*
//! the state. Fills are also stored individually so history survives, but the
//! authoritative balance on restart is the last snapshot.

use crate::types::{Asset, Fill, Side};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Tracks cash and per-asset position size. `serde`-serialisable so a caller can
/// snapshot and restore it verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Portfolio {
    cash: Decimal,
    positions: HashMap<String, Decimal>,
    /// Realized P&L since construction, in the cash asset.
    realized_pnl: Decimal,
    /// Volume-weighted average cost per unit, per asset symbol.
    avg_cost: HashMap<String, Decimal>,
}

impl Portfolio {
    pub fn new(starting_cash: Decimal) -> Self {
        Self {
            cash: starting_cash,
            positions: HashMap::new(),
            realized_pnl: dec!(0),
            avg_cost: HashMap::new(),
        }
    }

    pub fn cash(&self) -> Decimal {
        self.cash
    }

    pub fn realized_pnl(&self) -> Decimal {
        self.realized_pnl
    }

    pub fn position(&self, asset: &Asset) -> Decimal {
        self.positions
            .get(&asset.symbol)
            .copied()
            .unwrap_or(dec!(0))
    }

    /// Volume-weighted average entry cost for the current position, if any.
    pub fn avg_cost(&self, asset: &Asset) -> Option<Decimal> {
        self.avg_cost.get(&asset.symbol).copied()
    }

    /// Non-zero positions, as `(symbol, quantity)` pairs. Order is unspecified.
    pub fn positions(&self) -> impl Iterator<Item = (&str, Decimal)> {
        self.positions
            .iter()
            .filter(|(_, q)| **q != dec!(0))
            .map(|(s, q)| (s.as_str(), *q))
    }

    /// Number of distinct symbols with a non-zero position.
    pub fn open_position_count(&self) -> usize {
        self.positions().count()
    }

    /// Mark-to-market unrealized P&L across open positions, given a price
    /// oracle. Positions without a known price or a recorded average cost
    /// contribute nothing. Negative means an open loss.
    pub fn unrealized_pnl(&self, price_of: impl Fn(&str) -> Option<Decimal>) -> Decimal {
        let mut total = dec!(0);
        for (sym, qty) in self.positions() {
            if let (Some(px), Some(cost)) = (price_of(sym), self.avg_cost.get(sym).copied()) {
                total += qty * (px - cost);
            }
        }
        total
    }

    /// Mark-to-market equity given a price oracle for held assets.
    pub fn equity(&self, price_of: impl Fn(&str) -> Option<Decimal>) -> Decimal {
        let mut total = self.cash;
        for (sym, qty) in &self.positions {
            if let Some(px) = price_of(sym) {
                total += *qty * px;
            }
        }
        total
    }

    /// Apply a fill to the ledger. Updates cash, position, average cost, and
    /// realized P&L (on the closing portion of a sell).
    pub fn apply(&mut self, fill: &Fill) {
        self.cash += fill.cash_delta();
        let sym = fill.asset.symbol.clone();
        let prev_qty = self.positions.get(&sym).copied().unwrap_or(dec!(0));
        let prev_cost = self.avg_cost.get(&sym).copied().unwrap_or(dec!(0));

        match fill.side {
            Side::Buy => {
                let new_qty = prev_qty + fill.qty;
                if new_qty != dec!(0) {
                    let blended = (prev_qty * prev_cost + fill.qty * fill.price) / new_qty;
                    self.avg_cost.insert(sym.clone(), blended);
                }
                self.positions.insert(sym, new_qty);
            }
            Side::Sell => {
                let closed = fill.qty.min(prev_qty.max(dec!(0)));
                self.realized_pnl += closed * (fill.price - prev_cost) - fill.fee;
                let new_qty = prev_qty - fill.qty;
                if new_qty == dec!(0) {
                    self.avg_cost.remove(&sym);
                }
                self.positions.insert(sym, new_qty);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OrderId, Venue};
    use chrono::Utc;

    fn fill(side: Side, qty: Decimal, price: Decimal) -> Fill {
        Fill {
            order_id: OrderId::new("t"),
            asset: Asset::symbol("ROAR"),
            side,
            qty,
            price,
            fee: dec!(0),
            venue: Venue::Paper,
            at: Utc::now(),
        }
    }

    #[test]
    fn buy_then_sell_realizes_pnl() {
        let mut p = Portfolio::new(dec!(1000));
        p.apply(&fill(Side::Buy, dec!(10), dec!(20))); // -200
        assert_eq!(p.cash(), dec!(800));
        assert_eq!(p.position(&Asset::symbol("ROAR")), dec!(10));

        p.apply(&fill(Side::Sell, dec!(10), dec!(25))); // +250
        assert_eq!(p.cash(), dec!(1050));
        assert_eq!(p.realized_pnl(), dec!(50));
        assert_eq!(p.position(&Asset::symbol("ROAR")), dec!(0));
    }

    #[test]
    fn average_cost_blends_across_buys() {
        let mut p = Portfolio::new(dec!(10_000));
        p.apply(&fill(Side::Buy, dec!(10), dec!(10)));
        p.apply(&fill(Side::Buy, dec!(10), dec!(20)));
        // avg cost now 15; sell 20 @ 25 -> pnl = 20 * (25 - 15) = 200
        p.apply(&fill(Side::Sell, dec!(20), dec!(25)));
        assert_eq!(p.realized_pnl(), dec!(200));
    }

    #[test]
    fn unrealized_pnl_marks_open_positions_to_market() {
        let mut p = Portfolio::new(dec!(10_000));
        p.apply(&fill(Side::Buy, dec!(10), dec!(10))); // cost 10
                                                       // price 13 -> 10 * (13 - 10) = +30
        assert_eq!(
            p.unrealized_pnl(|s| (s == "ROAR").then_some(dec!(13))),
            dec!(30)
        );
        // no price known -> 0
        assert_eq!(p.unrealized_pnl(|_| None), dec!(0));
        assert_eq!(p.open_position_count(), 1);
    }

    #[test]
    fn json_round_trip_is_lossless() {
        let mut p = Portfolio::new(dec!(1000));
        p.apply(&fill(Side::Buy, dec!(3), dec!(11)));
        p.apply(&fill(Side::Buy, dec!(2), dec!(19)));
        p.apply(&fill(Side::Sell, dec!(1), dec!(25)));

        let json = serde_json::to_string(&p).unwrap();
        let restored: Portfolio = serde_json::from_str(&json).unwrap();
        assert_eq!(p, restored);
    }

    #[test]
    fn positions_iterator_skips_closed_positions() {
        let mut p = Portfolio::new(dec!(1000));
        p.apply(&fill(Side::Buy, dec!(5), dec!(10)));
        p.apply(&fill(Side::Sell, dec!(5), dec!(12)));
        assert_eq!(p.positions().count(), 0);
    }
}
