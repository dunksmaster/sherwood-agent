//! `sherwood` — the runner.
//!
//! This binary only ever wires strategies -> risk gate -> **paper** executor.
//! There is no code path here that reaches a real venue. To trade for real you
//! implement `sherwood_execution::Executor` against your venue and call the
//! runner from your own binary. See `docs/LIVE_EXECUTION.md`.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod config;
mod feed;
mod runner;
mod secrets_cmd;

use anyhow::Result;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn usage() -> ! {
    eprintln!(
        "sherwood — agentic paper-trading runner\n\n\
         USAGE:\n  \
         sherwood demo                 run the built-in paper scenario\n  \
         sherwood run <config.toml>    run against a config file (paper only)\n  \
         sherwood check <config.toml>  validate a config file and exit\n  \
         sherwood secrets <cmd>        manage the encrypted secret vault\n"
    );
    std::process::exit(2);
}

/// Install a Ctrl-C handler that sets the returned flag. A second Ctrl-C is not
/// caught, so the default handler still aborts a wedged run.
fn install_shutdown_handler() -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    let handler_flag = Arc::clone(&flag);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            tracing::warn!("Ctrl-C received — finishing the current tick, then stopping");
            handler_flag.store(true, Ordering::Relaxed);
        }
    });
    flag
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let shutdown = install_shutdown_handler();

    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("secrets") => secrets_cmd::run(args),
        Some("demo") => runner::demo(&shutdown).await,
        Some("run") => {
            let path: PathBuf = args.next().unwrap_or_else(|| usage()).into();
            let cfg = config::AppConfig::load(&path)?;
            runner::run(cfg, &shutdown).await
        }
        Some("check") => {
            let path: PathBuf = args.next().unwrap_or_else(|| usage()).into();
            let cfg = config::AppConfig::load(&path)?;
            println!(
                "ok: {} leaders, allowlist {} symbols, mode = paper",
                cfg.copytrade.leaders.len(),
                cfg.risk.allowlist.len()
            );
            Ok(())
        }
        _ => usage(),
    }
}
