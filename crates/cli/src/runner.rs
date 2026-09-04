//! Wires the pieces together for a paper run.
//!
//! Per tick from the [`PriceFeed`]:
//!   set price -> Decider -> Decision -> size into Order
//!     -> RiskGate::check -> PaperExecutor::execute -> Portfolio::apply
//!     -> publish events
//!
//! The loop is multi-asset: the feed defines the universe, one symbol per tick.
//! Equity and unrealized P&L are marked against the latest price seen for every
//! held symbol. The loop publishes [`Event`]s onto a [`Bus`]; subscribers
//! persist and log them. The portfolio *snapshot* is written to the store
//! directly â€” the loop owns that state.

use crate::config::AppConfig;
use crate::feed::{CsvFeed, SliceFeed};
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sherwood_chain::feed::{ChainFeed, ChainFeedConfig};
use sherwood_chain::{tokens, HttpClient};
use sherwood_core::{
    Asset, Clock, Decision, Fill, GateContext, MarketSnapshot, Order, OrderId, Portfolio,
    PriceFeed, RiskGate, Side, SystemClock, Tick, Venue,
};
use sherwood_decision::{
    AiConfig, AiDecider, AiProvider, Decider, DecisionContext, OpenAiCompatProvider, RuleConfig,
    RuleDecider,
};
use sherwood_events::{run_subscriber, Bus, Event, TracingSubscriber};
use sherwood_execution::{Executor, PaperExecutor};
use sherwood_secrets::resolve_ref;
use sherwood_store::{SqliteStore, Store, StoreSubscriber};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio::task::JoinHandle;

/// A two-symbol synthetic replay so `demo` (and `run` without a `feed_path`) do
/// something visible without a data source. Real deployments point at a CSV or,
/// later, a live feed.
fn demo_feed() -> Vec<Tick> {
    let roar = [
        dec!(0.00),
        dec!(0.03),
        dec!(0.07),
        dec!(0.12),
        dec!(0.19),
        dec!(0.24),
        dec!(0.10),
        dec!(-0.05),
    ];
    let hmni = [
        dec!(0.00),
        dec!(0.01),
        dec!(0.08),
        dec!(0.10),
        dec!(0.12),
        dec!(0.05),
        dec!(-0.05),
        dec!(-0.15),
    ];
    let base = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap_or_default();
    let mut ticks = Vec::with_capacity(roar.len() * 2);
    for (i, (r, h)) in roar.iter().zip(hmni).enumerate() {
        let at = base + Duration::minutes(i as i64);
        ticks.push(Tick {
            at,
            symbol: "ROAR".into(),
            price: dec!(100) * (dec!(1) + *r),
        });
        ticks.push(Tick {
            at,
            symbol: "HMNI".into(),
            price: dec!(10) * (dec!(1) + h),
        });
    }
    ticks
}

fn decision_tag(d: &Decision) -> &'static str {
    match d {
        Decision::Buy { .. } => "buy",
        Decision::Sell { .. } => "sell",
        Decision::Hold { .. } => "hold",
    }
}

/// Collected while a loop runs, for the backtest report. `None` in normal runs.
#[derive(Debug, Default, Clone)]
pub struct Recording {
    pub starting_cash: Decimal,
    /// Mark-to-market equity after each processed tick.
    pub equity_curve: Vec<Decimal>,
    pub fills: Vec<Fill>,
    pub final_equity: Decimal,
    pub realized_pnl: Decimal,
}

/// Everything a paper run needs except the decider (a separate argument so it
/// can be a trait object).
struct Run<'a> {
    label: &'a str,
    feed: Box<dyn PriceFeed>,
    starting_cash: Decimal,
    gate: RiskGate,
    shutdown: &'a AtomicBool,
    store: Option<Arc<SqliteStore>>,
    clock: &'a dyn Clock,
    /// When set, the loop records its equity curve and fills here.
    recorder: Option<&'a mut Recording>,
}

async fn run_loop(cfg: Run<'_>, decider: &dyn Decider) -> Result<()> {
    let Run {
        label,
        mut feed,
        starting_cash,
        gate,
        shutdown,
        store,
        clock,
        mut recorder,
    } = cfg;

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
    let mut interrupted = false;
    let mut latest: HashMap<String, Decimal> = HashMap::new();
    let mut prev: HashMap<String, Decimal> = HashMap::new();
    let mut last_order: HashMap<String, DateTime<Utc>> = HashMap::new();
    let mut tick_no: u32 = 0;

    while let Some(tick) = feed.next_tick() {
        if shutdown.load(Ordering::Relaxed) {
            tracing::warn!(tick = tick_no, "shutdown requested â€” stopping cleanly");
            interrupted = true;
            break;
        }

        let sym = tick.symbol.clone();
        let px = tick.price;
        let asset = tick.asset();
        exec.set_price(sym.clone(), px);

        let last_seen = prev.get(&sym).copied().unwrap_or(px);
        let change_24h = if last_seen > dec!(0) {
            (px - last_seen) / last_seen
        } else {
            dec!(0)
        };

        latest.insert(sym.clone(), px);
        let price_of = |s: &str| latest.get(s).copied();

        let pos = portfolio.position(&asset);
        let equity = portfolio.equity(price_of);
        let ctx = DecisionContext {
            snapshot: MarketSnapshot {
                asset: asset.clone(),
                price: px,
                change_24h,
                liquidity: Some(dec!(250_000)),
                at: tick.at,
            },
            position: pos,
            avg_cost: portfolio.avg_cost(&asset),
            position_fraction: if equity > dec!(0) {
                pos * px / equity
            } else {
                dec!(0)
            },
        };

        let decision = decider.decide(&ctx).await;
        tracing::info!(tick = tick_no, symbol = %sym, price = %px, ?decision, "decided");
        if !matches!(decision, Decision::Hold { .. }) {
            bus.publish(Event::Decided {
                tick: tick_no,
                price: px,
                decision: decision_tag(&decision).to_string(),
            });
        }

        if let Some(order) = size(&decision, &asset, pos, equity, px) {
            // Scope the immutable borrow of `portfolio` to the gate check.
            let gate_result = {
                let unrealized = portfolio.unrealized_pnl(price_of);
                let gctx = GateContext {
                    portfolio: &portfolio,
                    ref_price: Some(px),
                    equity,
                    unrealized_pnl: unrealized,
                    last_order_at: last_order.get(&sym).copied(),
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
                        if let Some(r) = recorder.as_mut() {
                            r.fills.push(fill.clone());
                        }
                        bus.publish(Event::OrderFilled(fill));
                    }
                    Err(e) => tracing::warn!(tick = tick_no, "executor rejected order: {e}"),
                },
                Err(e) => {
                    tracing::warn!(tick = tick_no, "risk gate blocked order: {e}");
                    bus.publish(Event::RiskRejected {
                        order_id: order.id.clone(),
                        symbol: order.asset.symbol.clone(),
                        reason: e.to_string(),
                    });
                }
            }
        }

        if let Some(r) = recorder.as_mut() {
            r.equity_curve
                .push(portfolio.equity(|s| latest.get(s).copied()));
        }

        prev.insert(sym, px);
        tick_no += 1;
    }

    let equity = portfolio.equity(|s| latest.get(s).copied());
    let state = if interrupted { "interrupted" } else { "done" };

    if let Some(r) = recorder.as_mut() {
        r.starting_cash = starting_cash;
        r.final_equity = equity;
        r.realized_pnl = portfolio.realized_pnl();
    }

    if let Some(s) = &store {
        s.save_portfolio(&portfolio).await?;
    }
    bus.publish(Event::RunEnded {
        label: label.to_string(),
        interrupted,
        cash: portfolio.cash(),
        realized_pnl: portfolio.realized_pnl(),
    });

    drop(bus);
    for h in subs {
        h.await?;
    }

    println!(
        "[{label}] {state}. cash={} open_positions={} equity={} realized_pnl={}",
        portfolio.cash(),
        portfolio.open_position_count(),
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
    let id = || {
        OrderId::new(format!(
            "d-{}-{}",
            asset.symbol,
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ))
    };
    match decision {
        Decision::Hold { .. } => None,
        Decision::Buy { fraction, reason } => {
            let qty = (equity * *fraction) / price;
            (qty > dec!(0)).then(|| Order {
                id: id(),
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
                id: id(),
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
    let gate = RiskGate::new(sherwood_core::RiskConfig {
        max_order_notional: dec!(10_000),
        max_position_fraction: dec!(0.50),
        ..Default::default()
    });
    run_loop(
        Run {
            label: "demo",
            feed: Box::new(SliceFeed::new(demo_feed())),
            starting_cash: dec!(1_000),
            gate,
            shutdown,
            store: None,
            clock: &SystemClock,
            recorder: None,
        },
        &RuleDecider::new(RuleConfig::default()),
    )
    .await
}

/// Build the feed `sherwood run` reads from: the live Robinhood Chain feed if
/// `[chain] enabled = true`, else the configured CSV, else the built-in demo.
/// **Still paper trading** either way — the feed only supplies prices; no
/// wallet, no signing, no order ever reaches the venue.
fn build_feed(cfg: &AppConfig) -> Result<Box<dyn PriceFeed>> {
    if cfg.chain.enabled {
        let client = HttpClient::new(cfg.chain.rpc_url.clone(), StdDuration::from_secs(30))
            .map_err(|e| anyhow!("chain feed: {e}"))?;
        let (_, denom_address, denom_decimals) = tokens::resolve(&cfg.chain.denom);
        let feed_cfg = ChainFeedConfig {
            denom_address,
            denom_decimals,
            poll_interval: StdDuration::from_secs(cfg.chain.poll_interval_secs),
            ..ChainFeedConfig::default()
        };
        return Ok(Box::new(ChainFeed::new(
            client,
            &cfg.chain.symbols,
            feed_cfg,
        )));
    }
    Ok(match &cfg.general.feed_path {
        Some(path) => Box::new(CsvFeed::open(path)?),
        None => Box::new(SliceFeed::new(demo_feed())),
    })
}

pub async fn run(cfg: AppConfig, shutdown: &AtomicBool) -> Result<()> {
    tracing::info!(
        "paper run: starting_cash={} decider={} state_path={:?} feed_path={:?} chain_enabled={}",
        cfg.general.starting_cash,
        cfg.general.decider,
        cfg.general.state_path,
        cfg.general.feed_path,
        cfg.chain.enabled,
    );
    let gate = RiskGate::new(cfg.risk.to_core());
    let decider = build_decider(&cfg)?;

    let store = match &cfg.general.state_path {
        Some(path) => Some(Arc::new(open_store(path).await?)),
        None => None,
    };

    let feed = build_feed(&cfg)?;

    run_loop(
        Run {
            label: "run",
            feed,
            starting_cash: cfg.general.starting_cash,
            gate,
            shutdown,
            store,
            clock: &SystemClock,
            recorder: None,
        },
        decider.as_ref(),
    )
    .await
}

/// Replay `cfg`'s feed through its decider once, deterministically, recording
/// the equity curve and every fill. No persistence, no bus subscribers beyond
/// tracing. Used by [`crate::backtest`].
pub async fn run_backtest(cfg: &AppConfig, shutdown: &AtomicBool) -> Result<Recording> {
    let decider = build_decider(cfg)?;
    let feed: Box<dyn PriceFeed> = match &cfg.general.feed_path {
        Some(path) => Box::new(CsvFeed::open(path)?),
        None => Box::new(SliceFeed::new(demo_feed())),
    };
    let mut rec = Recording::default();
    run_loop(
        Run {
            label: "backtest",
            feed,
            starting_cash: cfg.general.starting_cash,
            gate: RiskGate::new(cfg.risk.to_core()),
            shutdown,
            store: None,
            clock: &SystemClock,
            recorder: Some(&mut rec),
        },
        decider.as_ref(),
    )
    .await?;
    Ok(rec)
}

/// Build the decider named by `general.decider`. `"rule"` is self-contained;
/// `"ai"` resolves `ai.api_key` against the vault and wraps an
/// OpenAI-compatible provider. The config is already validated, so an unknown
/// name here is unreachable in practice â€” treated as an error regardless.
fn build_decider(cfg: &AppConfig) -> Result<Box<dyn Decider>> {
    match cfg.general.decider.as_str() {
        "rule" => Ok(Box::new(RuleDecider::new(RuleConfig::default()))),
        "ai" => {
            let vault = crate::secrets_cmd::open_vault()
                .context("opening the vault to resolve ai.api_key")?;
            let key = resolve_ref(&cfg.ai.api_key, &vault)?
                .ok_or_else(|| anyhow!("ai.api_key {:?} is not in the vault", cfg.ai.api_key))?;
            let provider = OpenAiCompatProvider::new(
                &cfg.ai.base_url,
                &cfg.ai.model,
                key.expose(),
                cfg.ai.temperature as f32,
                StdDuration::from_secs(cfg.ai.request_timeout_secs),
            )?;
            tracing::info!(
                provider = %provider.describe(),
                "ai decider enabled â€” advisory only, paper-only; RiskGate still decides"
            );
            let ai_cfg = AiConfig {
                max_tokens: cfg.ai.max_tokens,
                max_calls_per_run: cfg.ai.max_calls_per_run,
                universe: cfg.ai.universe.clone(),
            };
            Ok(Box::new(AiDecider::from_provider(
                Arc::new(provider),
                ai_cfg,
            )))
        }
        other => Err(anyhow!(
            "general.decider = {other:?} is not \"rule\" or \"ai\""
        )),
    }
}

async fn open_store(path: &Path) -> Result<SqliteStore> {
    Ok(SqliteStore::open(path).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sherwood_core::FixedClock;

    #[tokio::test]
    async fn stops_early_when_shutdown_is_set() {
        let flag = AtomicBool::new(true);
        demo(&flag).await.unwrap();
    }

    #[tokio::test]
    async fn completes_a_full_run_when_not_interrupted() {
        let flag = AtomicBool::new(false);
        demo(&flag).await.unwrap();
    }

    #[tokio::test]
    async fn run_loop_trades_multiple_symbols_through_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("state.db");
        let store = Arc::new(SqliteStore::open(&db).await.unwrap());
        let flag = AtomicBool::new(false);
        let clock = FixedClock(DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap());

        run_loop(
            Run {
                label: "t",
                feed: Box::new(SliceFeed::new(demo_feed())),
                starting_cash: dec!(1000),
                gate: RiskGate::new(sherwood_core::RiskConfig {
                    max_order_notional: dec!(10_000),
                    max_position_fraction: dec!(0.5),
                    ..Default::default()
                }),
                shutdown: &flag,
                store: Some(store.clone()),
                clock: &clock,
                recorder: None,
            },
            &RuleDecider::new(RuleConfig::default()),
        )
        .await
        .unwrap();

        let fills = store.fills().await.unwrap();
        let symbols: std::collections::HashSet<&str> =
            fills.iter().map(|f| f.asset.symbol.as_str()).collect();
        assert!(
            symbols.contains("ROAR") && symbols.contains("HMNI"),
            "both symbols traded, got {symbols:?}"
        );
        assert!(matches!(
            store.verify_audit_chain().await.unwrap(),
            sherwood_store::AuditVerification::Ok { .. }
        ));
    }

    #[tokio::test]
    async fn run_backtest_records_an_equity_curve_and_fills() {
        let dir = tempfile::tempdir().unwrap();
        let feed = dir.path().join("feed.csv");
        std::fs::write(
            &feed,
            "timestamp,symbol,price\n\
             2026-01-01T00:00:00Z,ROAR,100\n\
             2026-01-01T00:01:00Z,ROAR,108\n\
             2026-01-01T00:02:00Z,ROAR,120\n\
             2026-01-01T00:03:00Z,ROAR,135\n\
             2026-01-01T00:04:00Z,ROAR,95\n",
        )
        .unwrap();

        let cfg = AppConfig {
            general: crate::config::General {
                starting_cash: dec!(1000),
                mode: "paper".into(),
                state_path: None,
                feed_path: Some(feed),
                decider: "rule".into(),
            },
            risk: crate::config::RiskSection {
                max_order_notional: dec!(10_000),
                max_position_fraction: dec!(0.5),
                ..crate::config::RiskSection::default()
            },
            ai: Default::default(),
            copytrade: Default::default(),
            sniper: Default::default(),
            server: Default::default(),
            hook: Default::default(),
            chain: Default::default(),
        };

        let rec = run_backtest(&cfg, &AtomicBool::new(false)).await.unwrap();
        assert_eq!(rec.equity_curve.len(), 5); // one per tick
        assert_eq!(rec.starting_cash, dec!(1000));
        assert!(
            !rec.fills.is_empty(),
            "the momentum rule should have traded"
        );
    }

    #[tokio::test]
    async fn run_from_a_csv_feed_persists_and_verifies() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("state.db");
        let feed = dir.path().join("feed.csv");
        std::fs::write(
            &feed,
            "timestamp,symbol,price\n\
             2026-01-01T00:00:00Z,ROAR,100\n\
             2026-01-01T00:01:00Z,ROAR,103\n\
             2026-01-01T00:02:00Z,ROAR,110\n\
             2026-01-01T00:03:00Z,ROAR,120\n\
             2026-01-01T00:04:00Z,ROAR,132\n\
             2026-01-01T00:05:00Z,ROAR,95\n",
        )
        .unwrap();

        let cfg = AppConfig {
            general: crate::config::General {
                starting_cash: dec!(1000),
                mode: "paper".into(),
                state_path: Some(db.clone()),
                feed_path: Some(feed),
                decider: "rule".into(),
            },
            risk: crate::config::RiskSection {
                max_order_notional: dec!(10_000),
                max_position_fraction: dec!(0.5),
                ..crate::config::RiskSection::default()
            },
            ai: Default::default(),
            copytrade: Default::default(),
            sniper: Default::default(),
            server: Default::default(),
            hook: Default::default(),
            chain: Default::default(),
        };

        run(cfg, &AtomicBool::new(false)).await.unwrap();

        let s = SqliteStore::open(&db).await.unwrap();
        assert!(s.load_portfolio().await.unwrap().is_some());
        assert!(!s.fills().await.unwrap().is_empty());
        assert!(matches!(
            s.verify_audit_chain().await.unwrap(),
            sherwood_store::AuditVerification::Ok { .. }
        ));
        assert_eq!(s.audit_tail(1).await.unwrap()[0].kind, "run_end");
    }
}
