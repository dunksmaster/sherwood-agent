//! Copy-trading: watch a set of leader wallets, translate their observed swaps
//! into [`Order`]s sized for *your* book.
//!
//! The network side is abstracted behind [`TradeFeed`]. A real implementation
//! subscribes to chain logs (leader address, DEX router) or a data provider and
//! yields [`ObservedTrade`]s. This crate only handles the translation +
//! sizing + filtering, which is where the bugs and the risk live.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sherwood_core::{Asset, Order, OrderId, Side, Venue};
use std::collections::HashSet;

/// A trade made by a leader wallet that we observed on-chain / via a provider.
#[derive(Debug, Clone, PartialEq)]
pub struct ObservedTrade {
    pub leader: String,
    pub asset: Asset,
    pub side: Side,
    /// Base-asset quantity the leader traded.
    pub qty: Decimal,
    /// Execution price in the cash asset.
    pub price: Decimal,
    /// Leader's total portfolio value in the cash asset at trade time, if the
    /// feed can estimate it. Enables proportional (not absolute) mirroring.
    pub leader_equity: Option<Decimal>,
    pub tx: String,
    pub at: DateTime<Utc>,
}

/// Async source of leader trades.
#[async_trait]
pub trait TradeFeed: Send + Sync {
    /// Returns the next observed trade, or `None` when the feed ends.
    async fn next(&mut self) -> Option<ObservedTrade>;
}

#[derive(Debug, Clone)]
pub struct CopyConfig {
    /// Only mirror these leaders (lower-cased). Empty = mirror none.
    pub leaders: HashSet<String>,
    /// Sizing mode.
    pub sizing: Sizing,
    /// Skip trades whose notional is below this (leader dust).
    pub min_leader_notional: Decimal,
    /// Never build an order larger than this notional (a second cap on top of
    /// the risk gate, expressed in copy-trade terms).
    pub max_mirror_notional: Decimal,
    /// Slippage tolerance to stamp on generated orders.
    pub slippage: Decimal,
    /// Mirror sells even for assets we do not hold (no-op downstream) — usually
    /// false so we only act on exits we can actually make.
    pub mirror_unheld_sells: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum Sizing {
    /// Mirror a fixed fraction of the leader's traded quantity.
    FixedFraction(Decimal),
    /// Match the leader's *portfolio weight* change, scaled to our equity.
    /// Falls back to `FixedFraction(fallback)` when leader equity is unknown.
    ProportionalToEquity {
        our_equity: Decimal,
        fallback: Decimal,
    },
    /// Always spend a fixed cash notional per mirrored entry.
    FixedNotional(Decimal),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    UnknownLeader,
    BelowMinNotional,
    UnheldSell,
    ZeroQuantity,
}

pub struct CopyTrader {
    cfg: CopyConfig,
    seq: u64,
}

impl CopyTrader {
    pub fn new(cfg: CopyConfig) -> Self {
        Self { cfg, seq: 0 }
    }

    /// Translate one observed trade into an [`Order`], or explain the skip.
    ///
    /// `held_qty` is our current position in the asset (for sell filtering).
    pub fn mirror(&mut self, t: &ObservedTrade, held_qty: Decimal) -> Result<Order, SkipReason> {
        if !self.cfg.leaders.contains(&t.leader.to_lowercase()) {
            return Err(SkipReason::UnknownLeader);
        }
        if t.qty <= dec!(0) || t.price <= dec!(0) {
            return Err(SkipReason::ZeroQuantity);
        }
        if t.qty * t.price < self.cfg.min_leader_notional {
            return Err(SkipReason::BelowMinNotional);
        }
        if t.side == Side::Sell && held_qty <= dec!(0) && !self.cfg.mirror_unheld_sells {
            return Err(SkipReason::UnheldSell);
        }

        let mut qty = match self.cfg.sizing {
            Sizing::FixedFraction(f) => t.qty * f,
            Sizing::FixedNotional(n) => n / t.price,
            Sizing::ProportionalToEquity {
                our_equity,
                fallback,
            } => match t.leader_equity {
                Some(le) if le > dec!(0) => {
                    let leader_weight = (t.qty * t.price) / le;
                    (leader_weight * our_equity) / t.price
                }
                _ => t.qty * fallback,
            },
        };

        // For sells, never sell more than we hold.
        if t.side == Side::Sell {
            qty = qty.min(held_qty);
            if qty <= dec!(0) {
                return Err(SkipReason::UnheldSell);
            }
        }

        // Cap mirror notional.
        let notional = qty * t.price;
        if notional > self.cfg.max_mirror_notional {
            qty = self.cfg.max_mirror_notional / t.price;
        }

        self.seq += 1;
        Ok(Order {
            id: OrderId::new(format!("copy-{}-{}", &t.tx[..t.tx.len().min(10)], self.seq)),
            asset: t.asset.clone(),
            side: t.side,
            qty,
            limit_price: Some(t.price),
            max_slippage: self.cfg.slippage,
            venue: Venue::Paper,
            reason: format!("copytrade:{} tx:{}", t.leader, t.tx),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaders(xs: &[&str]) -> HashSet<String> {
        xs.iter().map(|s| s.to_lowercase()).collect()
    }

    fn trade(side: Side, qty: Decimal, price: Decimal) -> ObservedTrade {
        ObservedTrade {
            leader: "0xLEADER".into(),
            asset: Asset::symbol("ROAR"),
            side,
            qty,
            price,
            leader_equity: None,
            tx: "0xdeadbeef00".into(),
            at: Utc::now(),
        }
    }

    fn cfg(sizing: Sizing) -> CopyConfig {
        CopyConfig {
            leaders: leaders(&["0xleader"]),
            sizing,
            min_leader_notional: dec!(10),
            max_mirror_notional: dec!(1_000),
            slippage: dec!(0.01),
            mirror_unheld_sells: false,
        }
    }

    #[test]
    fn skips_unknown_leader() {
        let mut ct = CopyTrader::new(cfg(Sizing::FixedFraction(dec!(0.5))));
        let mut t = trade(Side::Buy, dec!(10), dec!(5));
        t.leader = "0xstranger".into();
        assert_eq!(
            ct.mirror(&t, dec!(0)).unwrap_err(),
            SkipReason::UnknownLeader
        );
    }

    #[test]
    fn fixed_fraction_halves_quantity() {
        let mut ct = CopyTrader::new(cfg(Sizing::FixedFraction(dec!(0.5))));
        let o = ct
            .mirror(&trade(Side::Buy, dec!(10), dec!(5)), dec!(0))
            .unwrap();
        assert_eq!(o.qty, dec!(5));
        assert_eq!(o.side, Side::Buy);
    }

    #[test]
    fn caps_mirror_notional() {
        let mut ct = CopyTrader::new(cfg(Sizing::FixedFraction(dec!(1))));
        // 500 * 5 = 2500 notional, cap is 1000 -> qty becomes 200
        let o = ct
            .mirror(&trade(Side::Buy, dec!(500), dec!(5)), dec!(0))
            .unwrap();
        assert_eq!(o.qty, dec!(200));
    }

    #[test]
    fn sell_is_clamped_to_held_quantity() {
        let mut ct = CopyTrader::new(cfg(Sizing::FixedFraction(dec!(1))));
        let o = ct
            .mirror(&trade(Side::Sell, dec!(100), dec!(5)), dec!(30))
            .unwrap();
        assert_eq!(o.qty, dec!(30));
    }

    #[test]
    fn proportional_sizing_matches_leader_weight() {
        let mut ct = CopyTrader::new(cfg(Sizing::ProportionalToEquity {
            our_equity: dec!(1_000),
            fallback: dec!(0.1),
        }));
        let mut t = trade(Side::Buy, dec!(100), dec!(2)); // 200 notional
        t.leader_equity = Some(dec!(10_000)); // leader put 2% in
        let o = ct.mirror(&t, dec!(0)).unwrap();
        // 2% of our 1000 = 20 notional / price 2 = 10 units
        assert_eq!(o.qty, dec!(10));
    }
}
