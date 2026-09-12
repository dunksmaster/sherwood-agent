//! `sherwood reconcile <config.toml> <tx_hash> <symbol> <buy|sell> <qty> <price> [fee]`
//!
//! Reads the receipt for a transaction **you** already broadcast with your
//! own tooling — this codebase never sends anything; see
//! [ADR-0007](../../../docs/adr/0007-no-broadcast-capability.md). If it
//! confirmed and a `[general] state_path` is configured, records it the same
//! way a paper fill is recorded: an appended fill, an updated portfolio
//! snapshot, an audit-chain row. Signs nothing, sends nothing — one read
//! (`eth_getTransactionReceipt`, polled) and, on success, a local write.

use crate::config::AppConfig;
use anyhow::{bail, Context, Result};
use rust_decimal::Decimal;
use sherwood_chain::HttpClient;
use sherwood_core::{Fill, Portfolio, Side};
use sherwood_reconcile::{wait_and_reconcile, ExpectedTrade, Outcome};
use sherwood_store::{SqliteStore, Store};
use std::path::PathBuf;
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_secs(3);
const TIMEOUT: Duration = Duration::from_secs(120);

pub fn usage() -> ! {
    eprintln!(
        "sherwood reconcile <config.toml> <tx_hash> <symbol> <buy|sell> <qty> <price> [fee]\n\n\
         Polls for the receipt of a transaction YOU already broadcast with your own\n\
         tooling (up to {TIMEOUT:?}). On success, if [general] state_path is set,\n\
         records the fill: appended fill, updated portfolio snapshot, audit row —\n\
         the same as a paper fill. This command signs and sends nothing.\n"
    );
    std::process::exit(2);
}

fn side_str(s: Side) -> &'static str {
    match s {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

pub async fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let cfg_path: PathBuf = args.next().unwrap_or_else(|| usage()).into();
    let tx_hash = args.next().unwrap_or_else(|| usage());
    let symbol = args.next().unwrap_or_else(|| usage());
    let side = match args
        .next()
        .unwrap_or_else(|| usage())
        .to_ascii_lowercase()
        .as_str()
    {
        "buy" => Side::Buy,
        "sell" => Side::Sell,
        other => bail!("side must be \"buy\" or \"sell\", got {other:?}"),
    };
    let qty: Decimal = args
        .next()
        .unwrap_or_else(|| usage())
        .parse()
        .context("qty must be a decimal number")?;
    let price: Decimal = args
        .next()
        .unwrap_or_else(|| usage())
        .parse()
        .context("price must be a decimal number")?;
    let fee: Decimal = match args.next() {
        Some(s) => s.parse().context("fee must be a decimal number")?,
        None => Decimal::ZERO,
    };

    let cfg = AppConfig::load(&cfg_path)?;
    let client = HttpClient::new(cfg.chain.rpc_url.clone(), Duration::from_secs(30))
        .context("connecting to the RPC")?;

    println!("polling for the receipt of {tx_hash} (up to {TIMEOUT:?}) …");
    let expected = ExpectedTrade {
        symbol: symbol.clone(),
        side,
        qty,
        price,
        fee,
    };
    let outcome = wait_and_reconcile(&client, &tx_hash, &expected, POLL_INTERVAL, TIMEOUT)
        .await
        .context("reading the transaction receipt")?;

    match outcome {
        Outcome::NotFound => {
            println!(
                "⏳ no receipt yet after {TIMEOUT:?} — it may still confirm. \
                 Re-run this command later with the same tx_hash; nothing has been recorded."
            );
        }
        Outcome::Reverted(r) => {
            println!(
                "⛔ {tx_hash} was mined but REVERTED (block {}, gas used {}). \
                 Nothing recorded — a reverted transaction spent gas but moved no funds.",
                r.block_number, r.gas_used
            );
        }
        Outcome::Confirmed { receipt, fill } => {
            println!(
                "✅ {tx_hash} confirmed in block {} (gas used {}): {} {qty} {symbol} @ {price}",
                receipt.block_number,
                receipt.gas_used,
                side_str(side)
            );
            match &cfg.general.state_path {
                Some(path) => {
                    let store = SqliteStore::open(path)
                        .await
                        .context("opening the state store")?;
                    record(&store, &fill, cfg.general.starting_cash).await?;
                    println!("recorded to {}", path.display());
                }
                None => println!(
                    "no [general] state_path configured — this fill is not recorded anywhere"
                ),
            }
        }
    }
    Ok(())
}

async fn record(store: &SqliteStore, fill: &Fill, starting_cash: Decimal) -> Result<()> {
    store.append_fill(fill).await.context("appending fill")?;
    let mut portfolio = match store
        .load_portfolio()
        .await
        .context("loading the portfolio snapshot")?
    {
        Some(p) => p,
        None => Portfolio::new(starting_cash),
    };
    portfolio.apply(fill);
    store
        .save_portfolio(&portfolio)
        .await
        .context("saving the portfolio snapshot")?;
    store
        .append_audit(
            "fill",
            serde_json::json!({
                "order_id": fill.order_id.0,
                "symbol": fill.asset.symbol,
                "side": side_str(fill.side),
                "qty": fill.qty.to_string(),
                "price": fill.price.to_string(),
                "fee": fill.fee.to_string(),
                "source": "reconcile",
            }),
        )
        .await
        .context("appending the audit row")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use sherwood_core::{Asset, OrderId, Venue};

    fn buy_fill() -> Fill {
        Fill {
            order_id: OrderId::new("reconcile-0xabc"),
            asset: Asset::symbol("NVDA"),
            side: Side::Buy,
            qty: dec!(1),
            price: dec!(200),
            fee: dec!(0.5),
            venue: Venue::DexRouter,
            at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn record_appends_the_fill_and_debits_a_fresh_portfolio() {
        let store = SqliteStore::open_in_memory().await.unwrap();
        record(&store, &buy_fill(), dec!(1000)).await.unwrap();

        assert_eq!(store.fills().await.unwrap().len(), 1);
        let p = store.load_portfolio().await.unwrap().unwrap();
        // starting_cash 1000, buy 1 NVDA @ 200 + 0.5 fee -> cash 799.5
        assert_eq!(p.cash(), dec!(799.5));
        assert_eq!(store.audit_tail(1).await.unwrap()[0].kind, "fill");
    }

    #[tokio::test]
    async fn record_applies_on_top_of_an_existing_snapshot() {
        let store = SqliteStore::open_in_memory().await.unwrap();
        store
            .save_portfolio(&Portfolio::new(dec!(500)))
            .await
            .unwrap();
        record(&store, &buy_fill(), dec!(999_999)).await.unwrap(); // starting_cash ignored — a snapshot exists
        let p = store.load_portfolio().await.unwrap().unwrap();
        assert_eq!(p.cash(), dec!(299.5)); // 500 - 200 - 0.5, not 999_999 - ...
    }
}
