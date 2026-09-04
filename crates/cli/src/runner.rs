//! Wires the pieces together for a paper run.
//!
//! Flow per tick:
//!   market snapshot -> Decider -> Decision -> size into Order
//!     -> RiskGate::check -> PaperExecutor::execute -> Portfolio::apply
//!     -> publish events
//!
//! The risk gate is unconditional. The run loop publishes [`Event`]s onto a
//! [`Bus`]; subscribers persist them and log them. The run loop itself never
//! calls the store to record a fill — it publishes, and the store's subscriber
//! writes. The portfolio *snapshot* is the exception: the loop owns that state,
//! so it writes it directly before announcing the run has ended.

use crate::config::AppConfig;
use anyhow::Result;
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sherwood_core::{
    Asset, Clock, Decision, GateContext, MarketSnapshot, Order, OrderId, Portfolio, RiskGate, Side,
    SystemClock, Venue,
};
use sherwood_decision::{Decider, DecisionContext, RuleConfig, RuleDecider};
use sherwood_events::{run_subscriber, Bus, Event, TracingSubscriber};
use sherwood_execution::{Executor, PaperExecutor};
use sherwood_store::{SqliteStore, Store, StoreSubscriber};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::task::JoinHandle;

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

fn decision_tag(d: &Decision) -> &'static str {
    match d {
        Decision::Buy { .. } => "buy",
        Decision::Sell { .. } => "sell",
        Decision::Hold { .. } => "hold",
    }
}

/// Everything a paper run needs except the decider (which stays a separate
/// argument so it can be a trait object).
struct Run<'a> {
    label: &'a str,
    asset: Asset,
    prices: Vec<Decimal>,
    starting_cash: Decimal,
    gate: RiskGate,
    shutdown: &'a AtomicBool,
    store: Option<Arc<SqliteStore>>,
    clock: &'a dyn Clock,
}

async fn run_loop(cfg: Run<'_>, decider: &dyn Decider) -> Result<()> {
    let Run {
        label,
        asset,
        prices,
        starting_cash,
        gate,
        shutdown,
        store,
        clock,
    } = cfg;

    let Some(&first) = prices.first() else {
        anyhow::bail!("price series is empty");
    };

    // Bus + subscribers. The tracing subscriber is always attached; the store
    // subscriber only when there is a store. Both are `await`ed on shutdown so
    // their writes flush before the process exits.
    let bus = Bus::new(1000);
    let mut subs: Vec<JoinHandle<()>> = vec![tokio::spawn(run_subscriber(
        bus.subscribe(),
        TracingSubscriber,
    ))];
    if let Some(s) = &store {
        subs.push(tokio::spawn(run_subscriber(
            bus.subscribe(),
            StoreSubscriber::new(s.clone()),
        )));
    }

    let mut portfolio = match &store {
        Some(s) => match s.load_portfolio().await? {
            Some(p) => {
                tracing::info!(cash = %p.cash(), "resumed from snapshot");
                p
            }
            None => Portfolio::new(starting_cash),
        },
        None => Portfolio::new(starting_cash),
    };

    let exec = PaperExecutor::default();
    let mut prev = first;
    let mut interrupted = false;
    let mut last_order: HashMap<String, chrono::DateTime<Utc>> = HashMap::new();

    for (i, px) in prices.iter().enumerate() {
        if shutdown.load(Ordering::Relaxed) {
            tracing::warn!(tick = i, "shutdown requested — stopping cleanly");
            interrupted = true;
            break;
        }
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
        if !matches!(decision, Decision::Hold { .. }) {
            bus.publish(Event::Decided {
                tick: i as u32,
                price: *px,
                decision: decision_tag(&decision).to_string(),
            });
        }

        if let Some(order) = size(&decision, &asset, pos, equity, *px) {
            // Scope the immutable borrow of `portfolio` to the gate check so the
            // fill path below can take `&mut portfolio`.
            let gate_result = {
                let unrealized = portfolio.unrealized_pnl(|s| (s == asset.symbol).then_some(*px));
                let gctx = GateContext {
                    portfolio: &portfolio,
                    ref_price: Some(*px),
                    equity,
                    unrealized_pnl: unrealized,
                    last_order_at: last_order.get(&order.asset.symbol).copied(),
                    now: clock.now(),
                };
                gate.check(&order, &gctx)
            };

            match gate_result {
                Ok(()) => match exec.execute(&order).await {
                    Ok(fill) => {
                        portfolio.apply(&fill);
                        last_order.insert(fill.asset.symbol.clone(), clock.now());
                        tracing::info!(
                            side = ?fill.side, qty = %fill.qty, price = %fill.price,
                            cash = %portfolio.cash(), "filled"
                        );
                        bus.publish(Event::OrderFilled(fill));
                    }
                    Err(e) => tracing::warn!(tick = i, "executor rejected order: {e}"),
                },
                Err(e) => {
                    tracing::warn!(tick = i, "risk gate blocked order: {e}");
                    bus.publish(Event::RiskRejected {
                        order_id: order.id.clone(),
                        symbol: order.asset.symbol.clone(),
                        reason: e.to_string(),
                    });
                }
            }
        }
        prev = *px;
    }

    // The loop invariant leaves `prev` holding the last processed price (the
    // series is non-empty, guarded above).
    let last = prev;
    let equity = portfolio.equity(|s| (s == asset.symbol).then_some(last));
    let state = if interrupted { "interrupted" } else { "done" };

    if let Some(s) = &store {
        s.save_portfolio(&portfolio).await?;
    }
    bus.publish(Event::RunEnded {
        label: label.to_string(),
        interrupted,
        cash: portfolio.cash(),
        realized_pnl: portfolio.realized_pnl(),
    });

    // Close the bus and wait for subscribers to drain their backlog.
    drop(bus);
    for h in subs {
        h.await?;
    }

    println!(
        "[{label}] {state}. cash={} position={} equity={} realized_pnl={}",
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

pub async fn demo(shutdown: &AtomicBool) -> Result<()> {
    let asset = Asset::symbol("ROAR");
    let gate = RiskGate::new(sherwood_core::RiskConfig {
        max_order_notional: dec!(10_000),
        max_position_fraction: dec!(0.50),
        ..Default::default()
    });
    run_loop(
        Run {
            label: "demo",
            asset,
            prices: synthetic_series(dec!(100)),
            starting_cash: dec!(1_000),
            gate,
            shutdown,
            store: None,
            clock: &SystemClock,
        },
        &RuleDecider::new(RuleConfig::default()),
    )
    .await
}

pub async fn run(cfg: AppConfig, shutdown: &AtomicBool) -> Result<()> {
    tracing::info!(
        "paper run: starting_cash={} leaders={} sniper_enabled={} state_path={:?}",
        cfg.general.starting_cash,
        cfg.copytrade.leaders.len(),
        cfg.sniper.enabled,
        cfg.general.state_path,
    );
    let asset = Asset::symbol("ROAR");
    let gate = RiskGate::new(cfg.risk.to_core());

    let store = match &cfg.general.state_path {
        Some(path) => Some(Arc::new(open_store(path).await?)),
        None => None,
    };

    run_loop(
        Run {
            label: "run",
            asset,
            prices: synthetic_series(dec!(100)),
            starting_cash: cfg.general.starting_cash,
            gate,
            shutdown,
            store,
            clock: &SystemClock,
        },
        &RuleDecider::new(RuleConfig::default()),
    )
    .await
}

async fn open_store(path: &Path) -> Result<SqliteStore> {
    Ok(SqliteStore::open(path).await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stops_early_when_shutdown_is_set() {
        let flag = AtomicBool::new(true); // already requested
        demo(&flag).await.unwrap();
    }

    #[tokio::test]
    async fn completes_a_full_run_when_not_interrupted() {
        let flag = AtomicBool::new(false);
        demo(&flag).await.unwrap();
    }

    #[tokio::test]
    async fn run_persists_state_and_audits_the_chain_via_the_bus() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("state.db");

        let cfg = AppConfig {
            general: crate::config::General {
                starting_cash: dec!(1000),
                mode: "paper".into(),
                state_path: Some(db.clone()),
            },
            risk: crate::config::RiskSection {
                max_order_notional: dec!(10_000),
                max_position_fraction: dec!(0.5),
                ..crate::config::RiskSection::default()
            },
            copytrade: Default::default(),
            sniper: Default::default(),
        };

        let flag = AtomicBool::new(false);
        run(cfg, &flag).await.unwrap();

        // Re-open independently and confirm the run left durable, verifiable state.
        let s = SqliteStore::open(&db).await.unwrap();
        let p = s.load_portfolio().await.unwrap().expect("a snapshot");
        assert!(
            p.realized_pnl() != dec!(0),
            "the demo series closes a position"
        );
        assert!(!s.fills().await.unwrap().is_empty(), "fills were recorded");
        assert!(
            matches!(
                s.verify_audit_chain().await.unwrap(),
                sherwood_store::AuditVerification::Ok { .. }
            ),
            "audit chain verifies"
        );
        let tail = s.audit_tail(1).await.unwrap();
        assert_eq!(tail[0].kind, "run_end");
    }
}
