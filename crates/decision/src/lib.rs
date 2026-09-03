//! The decision layer: given market context, produce a [`Decision`].
//!
//! Two deciders ship:
//!
//! * [`RuleDecider`] — transparent, deterministic threshold rules. Good default,
//!   fully testable, no external calls.
//! * [`AiDecider`] — wraps a user-supplied async closure that calls an LLM
//!   (e.g. the Claude API). This crate does **not** embed an API client or a
//!   prompt that recommends specific assets; you provide the call and the
//!   prompt. Its output is still funnelled through the same [`Decision`] type
//!   and therefore still clamped by the risk gate downstream.
//!
//! Whatever a decider returns is advisory. Nothing here places an order; the
//! runner turns a [`Decision`] into an [`Order`] and the risk gate has the
//! final say.

use async_trait::async_trait;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sherwood_core::{Decision, MarketSnapshot};
use std::future::Future;
use std::pin::Pin;

/// Context handed to a decider for one asset on one tick.
#[derive(Debug, Clone)]
pub struct DecisionContext {
    pub snapshot: MarketSnapshot,
    /// Current position size in base units (0 if flat).
    pub position: Decimal,
    /// Average entry cost for the current position, if any.
    pub avg_cost: Option<Decimal>,
    /// Fraction of equity this asset currently represents.
    pub position_fraction: Decimal,
}

#[async_trait]
pub trait Decider: Send + Sync {
    async fn decide(&self, ctx: &DecisionContext) -> Decision;
    fn name(&self) -> &'static str;
}

/// Deterministic momentum + take-profit / stop-loss rules.
#[derive(Debug, Clone)]
pub struct RuleConfig {
    /// Enter when 24h change is at or above this fraction and we are flat.
    pub entry_momentum: Decimal,
    /// Fraction of equity to allocate on entry.
    pub entry_fraction: Decimal,
    /// Exit the whole position when unrealized return reaches this.
    pub take_profit: Decimal,
    /// Exit the whole position when unrealized return falls to this (negative).
    pub stop_loss: Decimal,
    /// Require at least this much pool/book liquidity to act, if known.
    pub min_liquidity: Option<Decimal>,
}

impl Default for RuleConfig {
    fn default() -> Self {
        Self {
            entry_momentum: dec!(0.05),
            entry_fraction: dec!(0.05),
            take_profit: dec!(0.20),
            stop_loss: dec!(-0.10),
            min_liquidity: Some(dec!(25_000)),
        }
    }
}

pub struct RuleDecider {
    cfg: RuleConfig,
}

impl RuleDecider {
    pub fn new(cfg: RuleConfig) -> Self {
        Self { cfg }
    }
}

#[async_trait]
impl Decider for RuleDecider {
    async fn decide(&self, ctx: &DecisionContext) -> Decision {
        let s = &ctx.snapshot;

        if let (Some(min), Some(liq)) = (self.cfg.min_liquidity, s.liquidity) {
            if liq < min {
                return Decision::Hold {
                    reason: format!("liquidity {liq} below floor {min}"),
                };
            }
        }

        // Manage an open position first.
        if ctx.position > dec!(0) {
            if let Some(cost) = ctx.avg_cost {
                if cost > dec!(0) {
                    let ret = (s.price - cost) / cost;
                    if ret >= self.cfg.take_profit {
                        return Decision::Sell {
                            fraction: dec!(1),
                            reason: format!("take-profit hit ({ret:+} >= {})", self.cfg.take_profit),
                        };
                    }
                    if ret <= self.cfg.stop_loss {
                        return Decision::Sell {
                            fraction: dec!(1),
                            reason: format!("stop-loss hit ({ret:+} <= {})", self.cfg.stop_loss),
                        };
                    }
                }
            }
            return Decision::Hold { reason: "holding open position".into() };
        }

        // Flat: consider entry on momentum.
        if s.change_24h >= self.cfg.entry_momentum {
            return Decision::Buy {
                fraction: self.cfg.entry_fraction,
                reason: format!("24h momentum {:+} >= {}", s.change_24h, self.cfg.entry_momentum),
            };
        }

        Decision::Hold {
            reason: format!("no signal (24h {:+})", s.change_24h),
        }
    }

    fn name(&self) -> &'static str {
        "rule"
    }
}

type AiCall = Box<
    dyn Fn(&DecisionContext) -> Pin<Box<dyn Future<Output = Decision> + Send>> + Send + Sync,
>;

/// Adapter around a caller-provided LLM call. The closure owns the API client,
/// the prompt, and the parsing of the model's reply into a [`Decision`].
pub struct AiDecider {
    call: AiCall,
}

impl AiDecider {
    pub fn new<F, Fut>(call: F) -> Self
    where
        F: Fn(&DecisionContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Decision> + Send + 'static,
    {
        Self {
            call: Box::new(move |ctx| Box::pin(call(ctx))),
        }
    }
}

#[async_trait]
impl Decider for AiDecider {
    async fn decide(&self, ctx: &DecisionContext) -> Decision {
        (self.call)(ctx).await
    }

    fn name(&self) -> &'static str {
        "ai"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use sherwood_core::Asset;

    fn ctx(price: Decimal, change: Decimal, pos: Decimal, cost: Option<Decimal>) -> DecisionContext {
        DecisionContext {
            snapshot: MarketSnapshot {
                asset: Asset::symbol("ROAR"),
                price,
                change_24h: change,
                liquidity: Some(dec!(100_000)),
                at: Utc::now(),
            },
            position: pos,
            avg_cost: cost,
            position_fraction: dec!(0),
        }
    }

    #[tokio::test]
    async fn enters_on_momentum_when_flat() {
        let d = RuleDecider::new(RuleConfig::default());
        let out = d.decide(&ctx(dec!(10), dec!(0.08), dec!(0), None)).await;
        assert!(matches!(out, Decision::Buy { .. }));
    }

    #[tokio::test]
    async fn takes_profit_on_open_position() {
        let d = RuleDecider::new(RuleConfig::default());
        let out = d.decide(&ctx(dec!(13), dec!(0.0), dec!(5), Some(dec!(10)))).await;
        assert!(matches!(out, Decision::Sell { fraction, .. } if fraction == dec!(1)));
    }

    #[tokio::test]
    async fn stops_out_on_open_position() {
        let d = RuleDecider::new(RuleConfig::default());
        let out = d.decide(&ctx(dec!(8), dec!(0.0), dec!(5), Some(dec!(10)))).await;
        assert!(matches!(out, Decision::Sell { .. }));
    }

    #[tokio::test]
    async fn holds_on_thin_liquidity() {
        let d = RuleDecider::new(RuleConfig::default());
        let mut c = ctx(dec!(10), dec!(0.5), dec!(0), None);
        c.snapshot.liquidity = Some(dec!(1_000));
        assert!(matches!(d.decide(&c).await, Decision::Hold { .. }));
    }

    #[tokio::test]
    async fn ai_decider_delegates_to_closure() {
        let d = AiDecider::new(|_ctx| async {
            Decision::Hold { reason: "model said wait".into() }
        });
        assert!(matches!(d.decide(&ctx(dec!(1), dec!(1), dec!(0), None)).await, Decision::Hold { .. }));
    }
}
