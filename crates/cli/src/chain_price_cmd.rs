//! `sherwood chain-price [rpc-url] <token> [denominator]` — read a Stock
//! Token's live price off Robinhood Chain's deepest Uniswap v4 pool.
//!
//! Read-only: pool discovery + `StateView` reads, same as
//! [`sherwood_chain::univ4`]. No wallet, no signing. `token` / `denominator`
//! accept a known symbol or a raw address; `denominator` defaults to USDG.

use anyhow::{Context, Result};
use sherwood_chain::univ4::{self, Decimals};
use sherwood_chain::{EvmClient, HttpClient};
use std::time::Duration;

const DEFAULT_RPC: &str = "https://rpc.mainnet.chain.robinhood.com";
const POOL_MANAGER: &str = "0x8366a39cc670b4001a1121b8f6a443a643e40951";
const STATE_VIEW: &str = "0xf3334192d15450cdd385c8b70e03f9a6bd9e673b";
const USDG: &str = "0x5fc5360D0400a0Fd4f2af552ADD042D716F1d168";

/// (symbol, address, decimals) — Robinhood Chain Stock Tokens + the stables
/// they trade against. See `docs/ROBINHOOD-CHAIN.md`.
const KNOWN: &[(&str, &str, u8)] = &[
    ("NVDA", "0xd0601CE157Db5bdC3162BbaC2a2C8aF5320D9EEC", 18),
    ("TSLA", "0x322F0929c4625eD5bAd873c95208D54E1c003b2d", 18),
    ("AAPL", "0xaF3D76f1834A1d425780943C99Ea8A608f8a93f9", 18),
    ("MSFT", "0xe93237C50D904957Cf27E7B1133b510C669c2e74", 18),
    ("AMZN", "0x12f190a9F9d7D37a250758b26824B97CE941bF54", 18),
    ("GOOGL", "0x2e0847E8910a9732eB3fb1bb4b70a580ADAD4FE3", 18),
    ("META", "0xc0D6457C16Cc70d6790Dd43521C899C87ce02f35", 18),
    ("SPY", "0x117cc2133c37B721F49dE2A7a74833232B3B4C0C", 18),
    ("USDG", USDG, 6),
    ("WETH", "0x0Bd7D308f8E1639FAb988df18A8011f41EAcAD73", 18),
];

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

    let (token_symbol, token_addr, token_dec) = resolve(&token_arg);
    let (denom_symbol, _denom_addr, denom_dec) = resolve(&denom_arg);

    let client = HttpClient::new(rpc.clone(), Duration::from_secs(30))?;
    let head = client
        .block_number()
        .await
        .context("connecting to the RPC")?;
    // Filtering by an indexed currency keeps this cheap even over the whole
    // chain history (bloom-filtered at the node), unlike an unfiltered scan.
    let (price, pool) = univ4::read_price(
        &client,
        POOL_MANAGER,
        STATE_VIEW,
        &token_addr,
        Decimals {
            token: token_dec,
            denominator: denom_dec,
        },
        0,
        head,
    )
    .await
    .with_context(|| format!("reading {token_symbol} price"))?;

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

fn resolve(arg: &str) -> (String, String, u8) {
    KNOWN
        .iter()
        .find(|(sym, _, _)| sym.eq_ignore_ascii_case(arg))
        .map_or_else(
            || (arg.to_owned(), arg.to_owned(), 18),
            |(sym, addr, dec)| ((*sym).to_owned(), (*addr).to_owned(), *dec),
        )
}
