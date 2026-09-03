//! Sniper: react to newly-created liquidity pools / token listings and decide
//! whether to take a fast entry.
//!
//! The value this crate adds is the **safety screen** ([`RugScreen`]), not the
//! speed. Getting a transaction to land quickly is a venue/infra concern that
//! belongs in an [`Executor`](sherwood_execution) implementation. What belongs
//! here is the set of cheap, local checks that stop you from buying an obvious
//! honeypot the moment it appears.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sherwood_core::{Asset, Order, OrderId, Side, Venue};

/// A newly observed pool / listing.
#[derive(Debug, Clone, PartialEq)]
pub struct NewPoolEvent {
    pub asset: Asset,
    /// Quote-side liquidity at creation, in the cash asset.
    pub initial_liquidity: Decimal,
    /// Fraction of total token supply held by the deployer address `[0, 1]`.
    pub deployer_supply_fraction: Decimal,
    /// Is the LP token locked or burned?
    pub lp_locked: bool,
    /// Seconds the LP lock lasts (0 if not locked / unknown).
    pub lp_lock_secs: u64,
    /// Can the token contract still mint new supply?
    pub mint_enabled: bool,
    /// Can holders be blocked from selling (freeze / blacklist)?
    pub can_freeze: bool,
    /// Buy tax / sell tax as fractions, if known.
    pub buy_tax: Option<Decimal>,
    pub sell_tax: Option<Decimal>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct SniperConfig {
    pub min_initial_liquidity: Decimal,
    pub max_deployer_supply_fraction: Decimal,
    pub require_lp_locked: bool,
    pub min_lp_lock_secs: u64,
    pub reject_if_mintable: bool,
    pub reject_if_freezable: bool,
    /// Reject if buy or sell tax exceeds this fraction.
    pub max_tax: Decimal,
    /// Cash notional to spend per snipe.
    pub entry_notional: Decimal,
    /// Slippage tolerance stamped on the entry order (snipes need headroom).
    pub slippage: Decimal,
}

impl Default for SniperConfig {
    fn default() -> Self {
        Self {
            min_initial_liquidity: dec!(5_000),
            max_deployer_supply_fraction: dec!(0.15),
            require_lp_locked: true,
            min_lp_lock_secs: 60 * 60 * 24 * 30,
            reject_if_mintable: true,
            reject_if_freezable: true,
            max_tax: dec!(0.10),
            entry_notional: dec!(50),
            slippage: dec!(0.15),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RugFlag {
    LowLiquidity,
    DeployerHoldsTooMuch,
    LpNotLocked,
    LpLockTooShort,
    Mintable,
    Freezable,
    TaxTooHigh,
}

pub struct RugScreen {
    cfg: SniperConfig,
}

impl RugScreen {
    pub fn new(cfg: SniperConfig) -> Self {
        Self { cfg }
    }

    /// Run every check and collect all failures (not just the first) so the
    /// operator sees the full picture in logs.
    pub fn screen(&self, ev: &NewPoolEvent) -> Vec<RugFlag> {
        let c = &self.cfg;
        let mut flags = Vec::new();

        if ev.initial_liquidity < c.min_initial_liquidity {
            flags.push(RugFlag::LowLiquidity);
        }
        if ev.deployer_supply_fraction > c.max_deployer_supply_fraction {
            flags.push(RugFlag::DeployerHoldsTooMuch);
        }
        if c.require_lp_locked && !ev.lp_locked {
            flags.push(RugFlag::LpNotLocked);
        }
        if ev.lp_locked && ev.lp_lock_secs < c.min_lp_lock_secs {
            flags.push(RugFlag::LpLockTooShort);
        }
        if c.reject_if_mintable && ev.mint_enabled {
            flags.push(RugFlag::Mintable);
        }
        if c.reject_if_freezable && ev.can_freeze {
            flags.push(RugFlag::Freezable);
        }
        let tax = ev
            .buy_tax
            .unwrap_or(dec!(0))
            .max(ev.sell_tax.unwrap_or(dec!(0)));
        if tax > c.max_tax {
            flags.push(RugFlag::TaxTooHigh);
        }

        flags
    }

    /// Build an entry order iff the screen is clean. Returns the failing flags
    /// otherwise.
    pub fn entry_order(
        &self,
        ev: &NewPoolEvent,
        ref_price: Decimal,
    ) -> Result<Order, Vec<RugFlag>> {
        let flags = self.screen(ev);
        if !flags.is_empty() {
            return Err(flags);
        }
        if ref_price <= dec!(0) {
            return Err(vec![RugFlag::LowLiquidity]);
        }
        Ok(Order {
            id: OrderId::new(format!("snipe-{}", ev.asset.symbol)),
            asset: ev.asset.clone(),
            side: Side::Buy,
            qty: self.cfg.entry_notional / ref_price,
            limit_price: Some(ref_price),
            max_slippage: self.cfg.slippage,
            venue: Venue::Paper,
            reason: format!("sniper:new_pool liq={}", ev.initial_liquidity),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean_event() -> NewPoolEvent {
        NewPoolEvent {
            asset: Asset::onchain("PONS", "0xabc"),
            initial_liquidity: dec!(20_000),
            deployer_supply_fraction: dec!(0.05),
            lp_locked: true,
            lp_lock_secs: 60 * 60 * 24 * 90,
            mint_enabled: false,
            can_freeze: false,
            buy_tax: Some(dec!(0.02)),
            sell_tax: Some(dec!(0.02)),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn clean_event_passes_and_builds_order() {
        let s = RugScreen::new(SniperConfig::default());
        assert!(s.screen(&clean_event()).is_empty());
        let o = s.entry_order(&clean_event(), dec!(0.001)).unwrap();
        assert_eq!(o.side, Side::Buy);
        assert_eq!(o.qty, dec!(50) / dec!(0.001));
    }

    #[test]
    fn flags_mintable_and_unlocked_lp() {
        let s = RugScreen::new(SniperConfig::default());
        let mut ev = clean_event();
        ev.mint_enabled = true;
        ev.lp_locked = false;
        let flags = s.screen(&ev);
        assert!(flags.contains(&RugFlag::Mintable));
        assert!(flags.contains(&RugFlag::LpNotLocked));
        assert!(s.entry_order(&ev, dec!(0.001)).is_err());
    }

    #[test]
    fn flags_high_tax() {
        let s = RugScreen::new(SniperConfig::default());
        let mut ev = clean_event();
        ev.sell_tax = Some(dec!(0.40));
        assert!(s.screen(&ev).contains(&RugFlag::TaxTooHigh));
    }

    #[test]
    fn flags_short_lock() {
        let s = RugScreen::new(SniperConfig::default());
        let mut ev = clean_event();
        ev.lp_lock_secs = 60;
        assert!(s.screen(&ev).contains(&RugFlag::LpLockTooShort));
    }
}
