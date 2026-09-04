//! `sherwood serve` — start the local control-plane HTTP API.
//!
//! Loads the config, resolves (or on first run generates) the bearer token in
//! the vault, builds the `PreToolUse` hook state from `[risk]` + `[hook]`, and
//! serves until Ctrl-C.

use crate::config::AppConfig;
use crate::secrets_cmd;
use anyhow::{anyhow, Context, Result};
use sherwood_core::RiskGate;
use sherwood_server::auth::{ApiToken, TokenOrigin};
use sherwood_server::AppState;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub async fn run(cfg: AppConfig, shutdown: Arc<AtomicBool>) -> Result<()> {
    let addr: SocketAddr = cfg
        .server
        .bind
        .parse()
        .context("server.bind (already validated on load)")?;

    let vault = secrets_cmd::open_vault().context("opening the vault for the API token")?;
    let token_name = cfg
        .server
        .token_ref
        .strip_prefix("vault:")
        .unwrap_or(&cfg.server.token_ref)
        .trim();

    let (token, origin) =
        ApiToken::load_or_create(&vault, token_name).map_err(|e| anyhow!("{e}"))?;
    if origin == TokenOrigin::Created {
        eprintln!(
            "generated a new API token and stored it as `{token_name}` in the vault.\n\
             retrieve it with:  sherwood secrets get {token_name}"
        );
    }

    let allowlist = cfg.hook.to_allowlist();
    if allowlist.is_empty() {
        tracing::warn!(
            "[hook] has no tools configured — every PreToolUse call will be denied. \
             Add read_tools / place_tools / cancel_tools to the config."
        );
    }

    let state = AppState::new(token, RiskGate::new(cfg.risk.to_core()), allowlist);

    let flag = Arc::clone(&shutdown);
    let shutdown_fut = async move {
        while !flag.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        tracing::info!("shutdown requested — stopping the server");
    };

    sherwood_server::serve(addr, state, shutdown_fut)
        .await
        .context("server error")
}
