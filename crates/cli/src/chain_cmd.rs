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
use sherwood_chain::{EvmClient, HttpClient};
use std::time::Duration;

/// Robinhood Chain Stock Token addresses (ADR-0006 — cross-checked across two
/// independent public write-ups). A convenience for the CLI; not authoritative.
const KNOWN: &[(&str, &str)] = &[
    ("NVDA", "0xd0601CE157Db5bdC3162BbaC2a2C8aF5320D9EEC"),
    ("TSLA", "0x322F0929c4625eD5bAd873c95208D54E1c003b2d"),
    ("AAPL", "0xaF3D76f1834A1d425780943C99Ea8A608f8a93f9"),
    ("MSFT", "0xe93237C50D904957Cf27E7B1133b510C669c2e74"),
    ("AMZN", "0x12f190a9F9d7D37a250758b26824B97CE941bF54"),
    ("GOOGL", "0x2e0847E8910a9732eB3fb1bb4b70a580ADAD4FE3"),
    ("META", "0xc0D6457C16Cc70d6790Dd43521C899C87ce02f35"),
    ("SPY", "0x117cc2133c37B721F49dE2A7a74833232B3B4C0C"),
];

const DEFAULT_RPC: &str = "https://rpc.mainnet.chain.robinhood.com";

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
        let addr = resolve(t);
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

fn resolve(token: &str) -> String {
    KNOWN
        .iter()
        .find(|(sym, _)| sym.eq_ignore_ascii_case(token))
        .map_or_else(|| token.to_owned(), |(_, addr)| (*addr).to_owned())
}
