//! Venue selection for Robinhood Chain
//! ([ADR-0006](../../../docs/adr/0006-robinhood-chain-venue.md), v0.2.5).
//!
//! One question: given an order's notional, does it route to the **AMM**
//! (Uniswap v4, built by [`sherwood-dex`](../../dex/README.md)) or to an
//! **RFQ** venue? [`Router::choose`] answers it and hands back a
//! [`RouteChoice`] with a human-readable reason for the audit log.
//!
//! This crate is pure decision logic. It holds no RPC client, builds no
//! calldata, signs nothing, sends nothing — same boundary as every crate
//! below it in the v0.2 stack. It does **not** talk to an RFQ venue: none is
//! integrated or verified on Robinhood Chain yet (ADR-0006 lists the chain's
//! venues as "Uniswap v4 and RFQ", but only the AMM path is built). Until one
//! is, [`RouterConfig::rfq_available`] stays `false` and every order routes
//! to the AMM. The threshold logic is here now so that wiring an RFQ client
//! later is a config change, not a control-flow change.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use rust_decimal::Decimal;

/// Where an order should execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Venue {
    /// Uniswap v4 through the `UniversalRouter` — the path `sherwood-dex`
    /// builds.
    Amm,
    /// A request-for-quote venue. Selected only when one is both configured
    /// (`rfq_available`) and warranted by notional; nothing in this codebase
    /// talks to one yet.
    Rfq,
}

impl Venue {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Venue::Amm => "AMM",
            Venue::Rfq => "RFQ",
        }
    }
}

impl std::fmt::Display for Venue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A routing decision plus the reason for it — the reason is meant for the
/// audit log and the CLI, so it names the numbers it compared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteChoice {
    pub venue: Venue,
    pub reason: String,
}

/// How the router decides.
#[derive(Debug, Clone)]
pub struct RouterConfig {
    /// Orders whose notional is **at or above** this route to RFQ — but only
    /// when [`Self::rfq_available`] is `true`. `None` means notional alone
    /// never selects RFQ.
    pub rfq_min_notional: Option<Decimal>,
    /// Whether an RFQ venue is actually reachable. No RFQ client exists in
    /// this codebase, so this is `false` in every real config today; it is a
    /// field rather than a hard-coded `false` so the threshold path is
    /// testable and so enabling RFQ later needs no code change here.
    pub rfq_available: bool,
}

impl Default for RouterConfig {
    /// AMM-only: no threshold, no RFQ venue.
    fn default() -> Self {
        Self {
            rfq_min_notional: None,
            rfq_available: false,
        }
    }
}

/// Anything that can go wrong building or using a [`Router`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RouterError {
    #[error("rfq_min_notional must be > 0, got {0}")]
    NonPositiveThreshold(Decimal),
    #[error("rfq_available is set but rfq_min_notional is not — RFQ could never be selected")]
    RfqAvailableWithoutThreshold,
    #[error("order notional must be > 0, got {0}")]
    NonPositiveNotional(Decimal),
}

/// Routes orders to a venue by notional.
#[derive(Debug, Clone)]
pub struct Router {
    cfg: RouterConfig,
}

impl Router {
    /// Validate `cfg` and build a router.
    ///
    /// Rejects a non-positive threshold, and rejects `rfq_available` without
    /// a threshold (that config can never route to RFQ, so it is almost
    /// certainly a mistake — spell the AMM-only intent as
    /// `rfq_available: false`).
    pub fn new(cfg: RouterConfig) -> Result<Self, RouterError> {
        if let Some(t) = cfg.rfq_min_notional {
            if t <= Decimal::ZERO {
                return Err(RouterError::NonPositiveThreshold(t));
            }
        } else if cfg.rfq_available {
            return Err(RouterError::RfqAvailableWithoutThreshold);
        }
        Ok(Self { cfg })
    }

    /// An AMM-only router — no RFQ venue, no threshold. The common case
    /// today.
    #[must_use]
    pub fn amm_only() -> Self {
        Self {
            cfg: RouterConfig::default(),
        }
    }

    /// Choose a venue for an order of `notional` (quote-currency units, e.g.
    /// USDG). `notional` must be positive.
    pub fn choose(&self, notional: Decimal) -> Result<RouteChoice, RouterError> {
        if notional <= Decimal::ZERO {
            return Err(RouterError::NonPositiveNotional(notional));
        }

        let Some(threshold) = self.cfg.rfq_min_notional else {
            return Ok(RouteChoice {
                venue: Venue::Amm,
                reason: "no RFQ threshold configured — AMM is the only route".to_owned(),
            });
        };

        if !self.cfg.rfq_available {
            return Ok(RouteChoice {
                venue: Venue::Amm,
                reason: format!(
                    "notional {notional} would route to RFQ (threshold {threshold}), \
                     but no RFQ venue is configured — falling back to AMM"
                ),
            });
        }

        if notional >= threshold {
            Ok(RouteChoice {
                venue: Venue::Rfq,
                reason: format!("notional {notional} >= RFQ threshold {threshold}"),
            })
        } else {
            Ok(RouteChoice {
                venue: Venue::Amm,
                reason: format!("notional {notional} < RFQ threshold {threshold}"),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn amm_only_router_always_picks_amm() {
        let r = Router::amm_only();
        assert_eq!(r.choose(dec!(1)).unwrap().venue, Venue::Amm);
        assert_eq!(r.choose(dec!(1_000_000)).unwrap().venue, Venue::Amm);
    }

    #[test]
    fn threshold_without_an_rfq_venue_still_routes_to_amm() {
        let r = Router::new(RouterConfig {
            rfq_min_notional: Some(dec!(10_000)),
            rfq_available: false,
        })
        .unwrap();
        let big = r.choose(dec!(50_000)).unwrap();
        assert_eq!(big.venue, Venue::Amm);
        assert!(big.reason.contains("no RFQ venue is configured"));
    }

    #[test]
    fn with_an_rfq_venue_notional_selects_the_venue() {
        let r = Router::new(RouterConfig {
            rfq_min_notional: Some(dec!(10_000)),
            rfq_available: true,
        })
        .unwrap();
        assert_eq!(r.choose(dec!(9_999.99)).unwrap().venue, Venue::Amm);
        assert_eq!(r.choose(dec!(10_000)).unwrap().venue, Venue::Rfq); // boundary is inclusive
        assert_eq!(r.choose(dec!(10_001)).unwrap().venue, Venue::Rfq);
    }

    #[test]
    fn boundary_reason_names_both_numbers() {
        let r = Router::new(RouterConfig {
            rfq_min_notional: Some(dec!(10_000)),
            rfq_available: true,
        })
        .unwrap();
        assert_eq!(
            r.choose(dec!(10_000)).unwrap().reason,
            "notional 10000 >= RFQ threshold 10000"
        );
    }

    #[test]
    fn non_positive_notional_is_rejected() {
        let r = Router::amm_only();
        assert_eq!(
            r.choose(dec!(0)).unwrap_err(),
            RouterError::NonPositiveNotional(dec!(0))
        );
        assert_eq!(
            r.choose(dec!(-5)).unwrap_err(),
            RouterError::NonPositiveNotional(dec!(-5))
        );
    }

    #[test]
    fn non_positive_threshold_is_rejected() {
        let err = Router::new(RouterConfig {
            rfq_min_notional: Some(dec!(0)),
            rfq_available: false,
        })
        .unwrap_err();
        assert_eq!(err, RouterError::NonPositiveThreshold(dec!(0)));
    }

    #[test]
    fn rfq_available_without_a_threshold_is_rejected() {
        let err = Router::new(RouterConfig {
            rfq_min_notional: None,
            rfq_available: true,
        })
        .unwrap_err();
        assert_eq!(err, RouterError::RfqAvailableWithoutThreshold);
    }

    #[test]
    fn venue_display_is_stable() {
        assert_eq!(Venue::Amm.to_string(), "AMM");
        assert_eq!(Venue::Rfq.to_string(), "RFQ");
    }
}
