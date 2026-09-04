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

use anyhow::{Context, Result};
use sherwood_chain::tokens::{self, DEFAULT_RPC, POOL_MANAGER, STATE_VIEW};
use sherwood_chain::univ4::{self, Decimals};
use sherwood_chain::{EvmClient, HttpClient};
use sherwood_dex::{quote, ExactInputSingleSwap};
use std::time::Duration;

const UNIVERSAL_ROUTER: &str = "0x8876789976decbfcbbbe364623c63652db8c0904";

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

    let (token_symbol, token_addr, token_dec) = tokens::resolve(&token_arg);
    let (denom_symbol, denom_addr, denom_dec) = tokens::resolve(&denom_arg);

    let client = HttpClient::new(rpc.clone(), Duration::from_secs(30))?;
    let head = client
        .block_number()
        .await
        .context("connecting to the RPC")?;

    let best = univ4::find_best_pool(
        &client,
        univ4::Deployment {
            pool_manager: POOL_MANAGER,
            state_view: STATE_VIEW,
        },
        &token_addr,
        &denom_addr,
        0,
        head,
    )
    .await
    .with_context(|| format!("finding a {token_symbol}/{denom_symbol} pool"))?;

    let spot_price = univ4::quote_pool(
        &client,
        STATE_VIEW,
        &best.key,
        &token_addr,
        Decimals {
            token: token_dec,
            denominator: denom_dec,
        },
    )
    .await
    .context("reading the spot price")?;

    let zero_for_one = best.key.currency0.eq_ignore_ascii_case(&token_addr);
    // Rough expected output from the spot price (already denom-per-token,
    // regardless of which side is currency0/1) — a real caller should use a
    // live quoter for anything that matters; this is a simulation aid.
    let scale = 10u128.pow(u32::from(denom_dec));
    let expected_out_human = rust_decimal::Decimal::from(amount_in)
        / rust_decimal::Decimal::from(10u128.pow(u32::from(token_dec)))
        * spot_price;
    let expected_out_raw: u128 = (expected_out_human * rust_decimal::Decimal::from(scale))
        .try_into()
        .context("expected output does not fit u128")?;
    let amount_out_minimum = quote::amount_out_minimum(expected_out_raw, slippage_bps)?;

    let swap = ExactInputSingleSwap {
        pool: best.key,
        zero_for_one,
        amount_in,
        amount_out_minimum,
    };
    let deadline = quote::deadline_from_now(1800);
    let calldata = swap.execute_calldata(deadline)?;

    println!("{rpc}\nblock {head}");
    println!(
        "pool  fee {} / tickSpacing {} / liquidity {}",
        swap.pool.fee, swap.pool.tick_spacing, best.liquidity
    );
    println!("swap  {amount_in} {token_symbol} -> min {amount_out_minimum} {denom_symbol} (slippage {slippage_bps}bps)");
    println!(
        "calldata  {} bytes to UniversalRouter {UNIVERSAL_ROUTER}",
        calldata.len()
    );
    println!("calldata_hex {}", sherwood_chain::abi::to_hex(&calldata));
    println!("simulating as from={from} …\n");

    match client
        .call_from(
            &from,
            UNIVERSAL_ROUTER,
            &sherwood_chain::abi::to_hex(&calldata),
        )
        .await
    {
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
