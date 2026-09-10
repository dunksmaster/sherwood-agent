//! `sherwood route <notional> [rfq_min_notional] [rfq_available]` — print
//! which venue an order of that notional routes to (AMM or RFQ), and why.
//!
//! Pure decision logic ([`sherwood_router`]) — reads nothing, signs nothing,
//! sends nothing. No RFQ venue is integrated on Robinhood Chain, so unless
//! you pass `rfq_available` truthy this always prints `AMM`.

use anyhow::{Context, Result};
use rust_decimal::Decimal;
use sherwood_router::{Router, RouterConfig};

pub fn usage() -> ! {
    eprintln!(
        "sherwood route <notional> [rfq_min_notional] [rfq_available]\n\n\
         Prints the venue (AMM or RFQ) an order of <notional> routes to, and the\n\
         reason. <notional> and <rfq_min_notional> are decimals in quote-currency\n\
         units (e.g. USDG). <rfq_available> is 1|true|yes or 0|false|no (default\n\
         false) — there is no RFQ venue wired up, so the default always picks AMM.\n"
    );
    std::process::exit(2);
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "y" => Some(true),
        "0" | "false" | "no" | "n" => Some(false),
        _ => None,
    }
}

pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let notional: Decimal = args
        .next()
        .unwrap_or_else(|| usage())
        .parse()
        .context("notional must be a decimal number")?;

    let rfq_min_notional = match args.next() {
        Some(s) => Some(
            s.parse::<Decimal>()
                .context("rfq_min_notional must be a decimal number")?,
        ),
        None => None,
    };
    let rfq_available = match args.next() {
        Some(s) => parse_bool(&s)
            .with_context(|| format!("rfq_available: expected true/false, got {s:?}"))?,
        None => false,
    };

    let router = Router::new(RouterConfig {
        rfq_min_notional,
        rfq_available,
    })
    .context("invalid router config")?;

    let choice = router.choose(notional).context("routing this order")?;
    println!("venue  {}", choice.venue);
    println!("reason {}", choice.reason);
    Ok(())
}
