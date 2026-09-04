//! Deterministic backtest: replay a price feed through the configured decider
//! and the risk gate, then report the performance metrics.
//!
//! It reuses the paper [`run_loop`](crate::runner) exactly — same decider, same
//! gate, same paper executor — with a [`Recording`] threaded through to collect
//! the per-tick equity curve and every fill. Nothing is persisted.
//!
//! One deliberate simplification: `order_cooldown_secs` is forced to `0`. A
//! backtest replays in microseconds, so a wall-clock cooldown would block every
//! order after the first. Cooldown is a live-trading control; it is exercised in
//! `sherwood run`, not here.

use crate::config::AppConfig;
use crate::runner::{run_backtest, Recording};
use anyhow::{bail, Result};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sherwood_core::Side;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;

/// Round-trip performance summary.
#[derive(Debug, Clone)]
pub struct BacktestReport {
    pub label: String,
    pub starting_cash: Decimal,
    pub final_equity: Decimal,
    pub total_return_pct: Decimal,
    pub max_drawdown_pct: Decimal,
    pub fills: usize,
    pub closed_trades: usize,
    pub wins: usize,
    pub win_rate_pct: Decimal,
    pub gross_profit: Decimal,
    pub gross_loss: Decimal,
    /// `gross_profit / |gross_loss|`. `None` when there were no losing trades.
    pub profit_factor: Option<Decimal>,
    /// Mean realised P&L per closed trade.
    pub expectancy: Decimal,
}

/// Per-symbol average-cost lots, consumed to turn fills into closed-trade P&L.
#[derive(Default)]
struct Book {
    qty: Decimal,
    cost: Decimal, // total cost basis of the open position
}

/// Realised P&L of each closed trade (a sell that reduces or flattens a
/// position). A partial sell closes a proportional slice.
fn closed_trade_pnls(rec: &Recording) -> Vec<Decimal> {
    let mut books: HashMap<String, Book> = HashMap::new();
    let mut pnls = Vec::new();

    for f in &rec.fills {
        let b = books.entry(f.asset.symbol.clone()).or_default();
        match f.side {
            Side::Buy => {
                b.qty += f.qty;
                b.cost += f.qty * f.price + f.fee;
            }
            Side::Sell => {
                if b.qty <= dec!(0) {
                    // A sell with no recorded long position — skip rather than
                    // invent a basis.
                    continue;
                }
                let closed = f.qty.min(b.qty);
                let basis = b.cost * (closed / b.qty);
                let proceeds = closed * f.price - f.fee;
                pnls.push(proceeds - basis);
                b.qty -= closed;
                b.cost -= basis;
            }
        }
    }
    pnls
}

fn max_drawdown_pct(curve: &[Decimal]) -> Decimal {
    let mut peak = curve.first().copied().unwrap_or(dec!(0));
    let mut worst = dec!(0);
    for &e in curve {
        if e > peak {
            peak = e;
        }
        if peak > dec!(0) {
            let dd = (peak - e) / peak * dec!(100);
            if dd > worst {
                worst = dd;
            }
        }
    }
    worst
}

fn analyze(label: &str, rec: &Recording) -> BacktestReport {
    let pnls = closed_trade_pnls(rec);
    let closed = pnls.len();
    let wins = pnls.iter().filter(|p| **p > dec!(0)).count();
    let gross_profit: Decimal = pnls.iter().filter(|p| **p > dec!(0)).sum();
    let gross_loss: Decimal = pnls.iter().filter(|p| **p < dec!(0)).sum();
    let total_return_pct = if rec.starting_cash > dec!(0) {
        (rec.final_equity - rec.starting_cash) / rec.starting_cash * dec!(100)
    } else {
        dec!(0)
    };

    BacktestReport {
        label: label.to_string(),
        starting_cash: rec.starting_cash,
        final_equity: rec.final_equity,
        total_return_pct,
        max_drawdown_pct: max_drawdown_pct(&rec.equity_curve),
        fills: rec.fills.len(),
        closed_trades: closed,
        wins,
        win_rate_pct: if closed > 0 {
            Decimal::from(wins) / Decimal::from(closed) * dec!(100)
        } else {
            dec!(0)
        },
        gross_profit,
        gross_loss,
        profit_factor: (gross_loss < dec!(0)).then(|| gross_profit / gross_loss.abs()),
        expectancy: if closed > 0 {
            pnls.iter().sum::<Decimal>() / Decimal::from(closed)
        } else {
            dec!(0)
        },
    }
}

fn round2(d: Decimal) -> Decimal {
    d.round_dp(2)
}

fn print_report(r: &BacktestReport) {
    println!("\n── backtest: {} ──", r.label);
    println!("  starting cash     {}", round2(r.starting_cash));
    println!("  final equity      {}", round2(r.final_equity));
    println!("  total return      {} %", round2(r.total_return_pct));
    println!("  max drawdown      {} %", round2(r.max_drawdown_pct));
    println!("  fills             {}", r.fills);
    println!(
        "  closed trades     {} ({} win / {} loss)",
        r.closed_trades,
        r.wins,
        r.closed_trades - r.wins
    );
    println!("  win rate          {} %", round2(r.win_rate_pct));
    println!("  gross profit      {}", round2(r.gross_profit));
    println!("  gross loss        {}", round2(r.gross_loss));
    match r.profit_factor {
        Some(pf) => println!("  profit factor     {}", round2(pf)),
        None => println!("  profit factor     n/a (no losing trades)"),
    }
    println!("  expectancy/trade  {}", round2(r.expectancy));
}

pub async fn backtest(mut cfg: AppConfig, shutdown: &AtomicBool) -> Result<()> {
    if cfg.general.feed_path.is_none() {
        bail!(
            "backtest needs a price feed — set [general] feed_path to a CSV \
             (timestamp,symbol,price). The built-in demo feed is for `sherwood demo`."
        );
    }
    // See the module docs: cooldown is a live-trading control, not a backtest one.
    if cfg.risk.order_cooldown_secs != 0 {
        tracing::info!("backtest: forcing risk.order_cooldown_secs = 0");
        cfg.risk.order_cooldown_secs = 0;
    }

    let rec = run_backtest(&cfg, shutdown).await?;
    let report = analyze(&cfg.general.decider, &rec);
    print_report(&report);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use sherwood_core::{Asset, Fill, OrderId, Venue};

    fn fill(sym: &str, side: Side, qty: Decimal, price: Decimal) -> Fill {
        Fill {
            order_id: OrderId::new("t"),
            asset: Asset::symbol(sym),
            side,
            qty,
            price,
            fee: dec!(0),
            venue: Venue::Paper,
            at: Utc::now(),
        }
    }

    #[test]
    fn drawdown_is_peak_to_trough() {
        let curve = [dec!(100), dec!(120), dec!(90), dec!(110)];
        // peak 120 → trough 90 → 25%
        assert_eq!(max_drawdown_pct(&curve), dec!(25));
    }

    #[test]
    fn closed_trades_use_average_cost() {
        let rec = Recording {
            starting_cash: dec!(1000),
            final_equity: dec!(1030),
            realized_pnl: dec!(30),
            equity_curve: vec![dec!(1000), dec!(1030)],
            fills: vec![
                fill("ROAR", Side::Buy, dec!(2), dec!(100)),  // basis 200
                fill("ROAR", Side::Sell, dec!(2), dec!(115)), // proceeds 230 → +30
            ],
        };
        let r = analyze("rule", &rec);
        assert_eq!(r.closed_trades, 1);
        assert_eq!(r.wins, 1);
        assert_eq!(r.gross_profit, dec!(30));
        assert_eq!(r.expectancy, dec!(30));
        assert_eq!(r.profit_factor, None); // no losses
    }

    #[test]
    fn partial_sell_closes_a_proportional_slice() {
        let rec = Recording {
            starting_cash: dec!(1000),
            final_equity: dec!(1000),
            realized_pnl: dec!(0),
            equity_curve: vec![dec!(1000)],
            fills: vec![
                fill("ROAR", Side::Buy, dec!(4), dec!(100)), // basis 400
                fill("ROAR", Side::Sell, dec!(1), dec!(90)), // basis 100, proceeds 90 → -10
            ],
        };
        let r = analyze("rule", &rec);
        assert_eq!(r.closed_trades, 1);
        assert_eq!(r.wins, 0);
        assert_eq!(r.gross_loss, dec!(-10));
    }
}
