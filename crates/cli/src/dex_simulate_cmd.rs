//! `sherwood dex-simulate <from> <token> <amount_in_raw> [denom] [slippage_bps] [rpc]`
//!
//! Builds a single-hop exact-input `V4_SWAP` through the `UniversalRouter`
//! and **simulates** it with `eth_call` — no signing, no sending, costs
//! nothing, changes nothing on chain. This is the check
//! [`sherwood-dex`](../../dex/README.md) says to run before ever trusting
//! its calldata with real funds: does the constructed transaction actually
//! succeed from a real address, or does it revert?
//!
//! `from` must already hold `token` and have approved Permit2 for it (see
//! `sherwood-dex::permit2`) — a random address will correctly fail this
//! simulation for lack of balance/allowance, which is not a bug in the
//! encoding. `amount_in_raw` is in the token's base units (no decimal
//! scaling here — see `sherwood chain-price` for a token's decimals).
//!
//! The build-and-simulate logic itself lives in
//! [`crate::dex_preview`], shared with `POST /v1/dex/simulate` (v0.2.9).

use crate::dex_preview::{self, UNIVERSAL_ROUTER};
use anyhow::{Context, Result};
use sherwood_chain::tokens::DEFAULT_RPC;
use sherwood_chain::HttpClient;
use std::time::Duration;

pub fn usage() -> ! {
    eprintln!(
        "sherwood dex-simulate <from> <token> <amount_in_raw> [denom] [slippage_bps] [rpc]\n\n\
         Builds a V4_SWAP through the UniversalRouter and eth_call-simulates it.\n\
         Signs and sends nothing. `from` needs real balance + a Permit2 approval\n\
         for this to actually succeed; a random address correctly fails.\n"
    );
    std::process::exit(2);
}

pub async fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let from = args.next().unwrap_or_else(|| usage());
    let token_arg = args.next().unwrap_or_else(|| usage());
    let amount_in: u128 = args
        .next()
        .unwrap_or_else(|| usage())
        .parse()
        .context("amount_in_raw must be an integer (base units)")?;
    let denom_arg = args.next().unwrap_or_else(|| "USDG".to_owned());
    let slippage_bps: u32 = args
        .next()
        .map_or(Ok(50), |s| s.parse())
        .context("slippage_bps")?;
    let rpc = args.next().unwrap_or_else(|| DEFAULT_RPC.to_owned());

    let client = HttpClient::new(rpc.clone(), Duration::from_secs(30))?;
    let preview =
        dex_preview::build(&client, &token_arg, amount_in, &denom_arg, slippage_bps).await?;

    println!("{rpc}\nblock {}", preview.head);
    println!(
        "pool  fee {} / tickSpacing {} / liquidity {}",
        preview.pool_fee, preview.pool_tick_spacing, preview.pool_liquidity
    );
    println!(
        "swap  {} {} -> min {} {} (slippage {}bps)",
        preview.amount_in,
        preview.token_symbol,
        preview.amount_out_minimum,
        preview.denom_symbol,
        preview.slippage_bps
    );
    println!(
        "calldata  {} bytes to UniversalRouter {UNIVERSAL_ROUTER}",
        preview.calldata.len()
    );
    println!(
        "calldata_hex {}",
        sherwood_chain::abi::to_hex(&preview.calldata)
    );
    println!("simulating as from={from} …\n");

    match dex_preview::simulate(&client, &from, &preview).await {
        Ok(ret) => {
            println!("✅ eth_call succeeded — {} bytes returned", ret.len());
            println!("   the calldata is well-formed AND {from} has the balance + Permit2");
            println!("   allowance this swap needs. Still: sign and broadcast is a separate,");
            println!("   explicit step this crate does not take.");
        }
        Err(e) => {
            println!("⛔ eth_call reverted: {e}");
            println!("   Could be a real problem with the encoding, OR simply that {from}");
            println!("   lacks balance/allowance for this swap — both look like a revert here.");
        }
    }
    Ok(())
}
