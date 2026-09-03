//! A minimal in-memory portfolio ledger.

use crate::types::{Asset, Fill, Side};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;

/// Tracks cash and per-asset position size. Not persistent — the CLI is
/// responsible for snapshotting this to disk if it wants durability.
#[derive(Debug, Clone)]
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
}
