//! `sherwood chain-price [rpc-url] <token> [denominator]` — read a Stock
//! Token's live price off Robinhood Chain's deepest Uniswap v4 pool.
//!
//! Read-only: pool discovery + `StateView` reads, same as
//! [`sherwood_chain::univ4`]. No wallet, no signing. `token` / `denominator`
//! accept a known symbol or a raw address; `denominator` defaults to USDG.

use anyhow::{Context, Result};
use sherwood_chain::tokens::{self, DEFAULT_RPC, POOL_MANAGER, STATE_VIEW};
use sherwood_chain::univ4::{self, Decimals};
use sherwood_chain::{EvmClient, HttpClient};
use std::time::Duration;

pub async fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut positional = args.peekable();
    let rpc = match positional.peek() {
        Some(a) if a.starts_with("http://") || a.starts_with("https://") => {
            positional.next().unwrap_or_default()
        }
        _ => DEFAULT_RPC.to_owned(),
    };
    let token_arg = positional.next().unwrap_or_else(|| "NVDA".to_owned());
    let denom_arg = positional.next().unwrap_or_else(|| "USDG".to_owned());

    let (token_symbol, token_addr, token_dec) = tokens::resolve(&token_arg);
    let (denom_symbol, denom_addr, denom_dec) = tokens::resolve(&denom_arg);

    let client = HttpClient::new(rpc.clone(), Duration::from_secs(30))?;
    let head = client
        .block_number()
        .await
        .context("connecting to the RPC")?;
    // Filtering by both currencies keeps this cheap even over the whole chain
    // history (bloom-filtered at the node) and — unlike filtering by `token`
    // alone — only ever considers a pool actually paired against `denom`.
    let deployment = univ4::Deployment {
        pool_manager: POOL_MANAGER,
        state_view: STATE_VIEW,
    };
    let (price, pool) = univ4::read_price(
        &client,
        deployment,
        &token_addr,
        &denom_addr,
        Decimals {
            token: token_dec,
            denominator: denom_dec,
        },
        0,
        head,
    )
    .await
    .with_context(|| format!("reading {token_symbol}/{denom_symbol} price"))?;

    println!("{rpc}\nblock {head}\n");
    println!("{token_symbol} ({token_addr})");
    println!(
        "  pool              fee {} / tickSpacing {} / hooks {}",
        pool.key.fee, pool.key.tick_spacing, pool.key.hooks
    );
    println!("  liquidity         {}", pool.liquidity);
    println!("  price             {price} {denom_symbol} per {token_symbol}");
    Ok(())
}
