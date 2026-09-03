//! Wires the pieces together for a paper run.
//!
//! Flow per tick:
//!   market snapshot -> Decider -> Decision -> size into Order
//!     -> RiskGate::check -> PaperExecutor::execute -> Portfolio::apply
//!
//! The risk gate is unconditional. A decision that the gate rejects is logged
//! and dropped.

use crate::config::AppConfig;
use anyhow::Result;
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sherwood_core::{
    Asset, Decision, MarketSnapshot, Order, OrderId, Portfolio, RiskGate, Side, Venue,
};
use sherwood_decision::{Decider, DecisionContext, RuleConfig, RuleDecider};
use sherwood_execution::{Executor, PaperExecutor};

/// A tiny synthetic price path so `demo` and `run` do something visible without
/// a network feed. Real deployments replace this with a live data source.
fn synthetic_series(base: Decimal) -> Vec<Decimal> {
    let steps = [
        dec!(0.00),
        dec!(0.03),
        dec!(0.07), // momentum entry trips here
        dec!(0.12),
        dec!(0.19),
        dec!(0.24), // take-profit
        dec!(0.10),
        dec!(-0.05),
    ];
    steps.iter().map(|d| base * (dec!(1) + *d)).collect()
}

async fn run_loop(
    label: &str,
    asset: Asset,
    prices: Vec<Decimal>,
    starting_cash: Decimal,
    gate: RiskGate,
    decider: impl Decider,
) -> Result<()> {
    let mut portfolio = Portfolio::new(starting_cash);
    let exec = PaperExecutor::default();
    let mut prev = prices[0];

    for (i, px) in prices.iter().enumerate() {
        exec.set_price(asset.symbol.clone(), *px);
        let change_24h = if prev > dec!(0) {
            (*px - prev) / prev
        } else {
            dec!(0)
        };

        let pos = portfolio.position(&asset);
        let equity = portfolio.equity(|s| (s == asset.symbol).then_some(*px));
        let ctx = DecisionContext {
            snapshot: MarketSnapshot {
                asset: asset.clone(),
                price: *px,
                change_24h,
                liquidity: Some(dec!(250_000)),
                at: Utc::now(),
            },
            position: pos,
            avg_cost: portfolio.avg_cost(&asset),
            position_fraction: if equity > dec!(0) {
                pos * *px / equity
            } else {
                dec!(0)
            },
        };

        let decision = decider.decide(&ctx).await;
        tracing::info!(tick = i, price = %px, ?decision, "decided");

        if let Some(order) = size(&decision, &asset, pos, equity, *px) {
            match gate.check(&order, &portfolio, Some(*px), equity) {
                Ok(()) => match exec.execute(&order).await {
                    Ok(fill) => {
                        portfolio.apply(&fill);
                        tracing::info!(
                            side = ?fill.side, qty = %fill.qty, price = %fill.price,
                            cash = %portfolio.cash(), "filled"
                        );
                    }
                    Err(e) => tracing::warn!("exec rejected: {e}"),
                },
                Err(e) => tracing::warn!("risk gate blocked order: {e}"),
            }
        }
        prev = *px;
    }

    let last = *prices.last().unwrap();
    let equity = portfolio.equity(|s| (s == asset.symbol).then_some(last));
    println!(
        "[{label}] done. cash={} position={} equity={} realized_pnl={}",
        portfolio.cash(),
        portfolio.position(&asset),
        equity,
        portfolio.realized_pnl()
    );
    Ok(())
}

/// Convert a [`Decision`] plus context into a concrete [`Order`], or `None` for
/// Hold / nothing-to-do.
fn size(
    decision: &Decision,
    asset: &Asset,
    position: Decimal,
    equity: Decimal,
    price: Decimal,
) -> Option<Order> {
    if price <= dec!(0) {
        return None;
    }
    match decision {
        Decision::Hold { .. } => None,
        Decision::Buy { fraction, reason } => {
            let notional = equity * *fraction;
            let qty = notional / price;
            (qty > dec!(0)).then(|| Order {
                id: OrderId::new(format!("d-{}", Utc::now().timestamp_millis())),
                asset: asset.clone(),
                side: Side::Buy,
                qty,
                limit_price: Some(price),
                max_slippage: dec!(0.02),
                venue: Venue::Paper,
                reason: format!("decision: {reason}"),
            })
        }
        Decision::Sell { fraction, reason } => {
            let qty = position * *fraction;
            (qty > dec!(0)).then(|| Order {
                id: OrderId::new(format!("d-{}", Utc::now().timestamp_millis())),
                asset: asset.clone(),
                side: Side::Sell,
                qty,
                limit_price: Some(price),
                max_slippage: dec!(0.02),
                venue: Venue::Paper,
                reason: format!("decision: {reason}"),
            })
        }
    }
}

pub async fn demo() -> Result<()> {
    let asset = Asset::symbol("ROAR");
    let gate = RiskGate::new(sherwood_core::RiskConfig {
        max_order_notional: dec!(10_000),
        max_position_fraction: dec!(0.50),
        ..Default::default()
    });
    run_loop(
        "demo",
        asset,
        synthetic_series(dec!(100)),
        dec!(1_000),
        gate,
        RuleDecider::new(RuleConfig::default()),
    )
    .await
}

pub async fn run(cfg: AppConfig) -> Result<()> {
    tracing::info!(
        "paper run: starting_cash={} leaders={} sniper_enabled={}",
        cfg.general.starting_cash,
        cfg.copytrade.leaders.len(),
        cfg.sniper.enabled
    );
    let asset = Asset::symbol("ROAR");
    let gate = RiskGate::new(cfg.risk.to_core());
    run_loop(
        "run",
        asset,
        synthetic_series(dec!(100)),
        cfg.general.starting_cash,
        gate,
        RuleDecider::new(RuleConfig::default()),
    )
    .await
}
