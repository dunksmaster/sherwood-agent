//! `sherwood chain-probe <rpc-url> [token ...]` — the on-chain pre-flight from
//! [ADR-0006](../../../docs/adr/0006-robinhood-chain-venue.md), in Rust.
//!
//! Read-only. It connects to an EVM RPC, samples recent `Transfer` traffic, and
//! *simulates* (`eth_call`, never a send) a transfer to a fresh un-onboarded
//! address to check the token is not allowlist-gated. Nothing is signed.
//!
//! `token` may be an address or one of the known Robinhood Chain symbols.

use anyhow::{Context, Result};
use sherwood_chain::probe::{check_transfer_open, ProbeOptions, Verdict};
use sherwood_chain::tokens::{self, DEFAULT_RPC};
use sherwood_chain::{EvmClient, HttpClient};
use std::time::Duration;

pub async fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut positional = args.peekable();
    // First arg is the RPC URL unless it looks like a token; then default.
    let rpc = match positional.peek() {
        Some(a) if a.starts_with("http://") || a.starts_with("https://") => {
            positional.next().unwrap_or_default()
        }
        _ => DEFAULT_RPC.to_owned(),
    };
    let mut targets: Vec<String> = positional.collect();
    if targets.is_empty() {
        targets.push("NVDA".to_owned());
    }

    let client = HttpClient::new(rpc.clone(), Duration::from_secs(30))?;
    let chain_id = client.chain_id().await.context("connecting to the RPC")?;
    let block = client.block_number().await?;
    println!("{rpc}\nchainId {chain_id}  block {block}\n");

    let mut all_permissionless = true;
    for t in &targets {
        let (_, addr, _) = tokens::resolve(t);
        match check_transfer_open(&client, &addr, &ProbeOptions::default()).await {
            Ok(report) => {
                println!("{report}\n");
                all_permissionless &= matches!(report.verdict, Verdict::Permissionless);
            }
            Err(e) => {
                println!("{t}: probe error: {e}\n");
                all_permissionless = false;
            }
        }
    }

    if all_permissionless {
        println!("OK — every probed token's transfers are permissionless at the contract.");
        Ok(())
    } else {
        anyhow::bail!("a probed token is restricted or the probe was inconclusive (see above)");
    }
}
