//! `sherwood` — the runner.
//!
//! This binary only ever wires strategies -> risk gate -> **paper** executor.
//! There is no code path here that reaches a real venue. To trade for real you
//! implement `sherwood_execution::Executor` against your venue and call the
//! runner from your own binary. See `docs/LIVE_EXECUTION.md`.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod backtest;
mod backup_cmd;
mod config;
mod feed;
mod runner;
mod secrets_cmd;
mod serve_cmd;

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
         sherwood backtest <config.toml>  replay the feed and print performance metrics\n  \
         sherwood serve <config.toml>  start the local control-plane HTTP API\n  \
         sherwood check <config.toml>  validate a config file and exit\n  \
         sherwood backup <config.toml> <dir>    copy the state DB + vault into <dir>\n  \
         sherwood restore <config.toml> <backup-dir> [--force]   copy them back\n  \
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

/// Console logging always; a daily-rolling JSON file too when `$SHERWOOD_LOG_DIR`
/// is set (7-day default retention — old files are the operator's to prune, or
/// `logrotate`'s). Returns a guard that must live for the process.
fn init_tracing() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    let console = tracing_subscriber::fmt::layer();

    let (file_layer, guard) = match std::env::var_os("SHERWOOD_LOG_DIR") {
        Some(dir) => {
            let appender = tracing_appender::rolling::daily(dir, "sherwood.log");
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let layer = tracing_subscriber::fmt::layer()
                .json()
                .with_writer(writer)
                .with_ansi(false);
            (Some(layer), Some(guard))
        }
        None => (None, None),
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(console)
        .with(file_layer)
        .init();
    guard
}

#[tokio::main]
async fn main() -> Result<()> {
    let _log_guard = init_tracing();

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
        Some("backtest") => {
            let path: PathBuf = args.next().unwrap_or_else(|| usage()).into();
            let cfg = config::AppConfig::load(&path)?;
            backtest::backtest(cfg, &shutdown).await
        }
        Some("serve") => {
            let path: PathBuf = args.next().unwrap_or_else(|| usage()).into();
            let cfg = config::AppConfig::load(&path)?;
            serve_cmd::run(cfg, Arc::clone(&shutdown)).await
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
        Some("backup") => {
            let path: PathBuf = args.next().unwrap_or_else(|| usage()).into();
            let dest: PathBuf = args.next().unwrap_or_else(|| usage()).into();
            let cfg = config::AppConfig::load(&path)?;
            backup_cmd::backup(&cfg, &dest)
        }
        Some("restore") => {
            let path: PathBuf = args.next().unwrap_or_else(|| usage()).into();
            let from: PathBuf = args.next().unwrap_or_else(|| usage()).into();
            let force = args.next().as_deref() == Some("--force");
            let cfg = config::AppConfig::load(&path)?;
            backup_cmd::restore(&cfg, &from, force)
        }
        _ => usage(),
    }
}
