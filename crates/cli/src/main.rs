//! `sherwood` — the runner.
//!
//! This binary only ever wires strategies -> risk gate -> **paper** executor.
//! There is no code path here that reaches a real venue. To trade for real you
//! implement `sherwood_execution::Executor` against your venue and call the
//! runner from your own binary. See `docs/LIVE_EXECUTION.md`.

mod config;
mod runner;

use anyhow::Result;
use std::path::PathBuf;

fn usage() -> ! {
    eprintln!(
        "sherwood — agentic paper-trading runner\n\n\
         USAGE:\n  \
         sherwood demo                 run the built-in paper scenario\n  \
         sherwood run <config.toml>    run against a config file (paper only)\n  \
         sherwood check <config.toml>  validate a config file and exit\n"
    );
    std::process::exit(2);
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("demo") => runner::demo().await,
        Some("run") => {
            let path: PathBuf = args.next().unwrap_or_else(|| usage()).into();
            let cfg = config::AppConfig::load(&path)?;
            runner::run(cfg).await
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
