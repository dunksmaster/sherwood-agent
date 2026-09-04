//! `sherwood serve` — start the local control-plane HTTP API.
//!
//! Loads the config, resolves (or on first run generates) the role tokens in
//! the vault, builds the `PreToolUse` hook state from `[risk]` + `[hook]`, and
//! serves until Ctrl-C.

use crate::config::AppConfig;
use crate::secrets_cmd;
use anyhow::{anyhow, Context, Result};
use sherwood_core::RiskGate;
use sherwood_secrets::SecretsVault;
use sherwood_server::auth::{ApiToken, TokenOrigin, TokenSet};
use sherwood_server::AppState;
use sherwood_store::SqliteStore;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn vault_name(reference: &str) -> &str {
    reference.strip_prefix("vault:").unwrap_or(reference).trim()
}

/// Load a role token, generating it into the vault on first use. Announces a
/// freshly minted token once, on stderr.
fn resolve_token(vault: &dyn SecretsVault, reference: &str, role: &str) -> Result<ApiToken> {
    let name = vault_name(reference);
    let (token, origin) = ApiToken::load_or_create(vault, name).map_err(|e| anyhow!("{e}"))?;
    if origin == TokenOrigin::Created {
        eprintln!(
            "generated a new {role} API token, stored as `{name}` in the vault.\n\
             retrieve it with:  sherwood secrets get {name}"
        );
    }
    Ok(token)
}

pub async fn run(cfg: AppConfig, shutdown: Arc<AtomicBool>) -> Result<()> {
    let addr: SocketAddr = cfg
        .server
        .bind
        .parse()
        .context("server.bind (already validated on load)")?;

    let vault = secrets_cmd::open_vault().context("opening the vault for the API tokens")?;

    let admin = resolve_token(&vault, &cfg.server.token_ref, "admin")?;
    let operator = cfg
        .server
        .operator_token_ref
        .as_deref()
        .map(|r| resolve_token(&vault, r, "operator"))
        .transpose()?;
    let viewer = cfg
        .server
        .viewer_token_ref
        .as_deref()
        .map(|r| resolve_token(&vault, r, "viewer"))
        .transpose()?;
    let tokens = TokenSet::new(admin, operator, viewer);

    let allowlist = cfg.hook.to_allowlist();
    if allowlist.is_empty() {
        tracing::warn!(
            "[hook] has no tools configured — every PreToolUse call will be denied. \
             Add read_tools / place_tools / cancel_tools to the config."
        );
    }
    if cfg.server.allow_live {
        tracing::warn!("[server] allow_live = true — an admin can switch this server to LIVE mode");
    }

    let store = match &cfg.general.state_path {
        Some(path) => {
            let s = SqliteStore::open(path)
                .await
                .with_context(|| format!("opening the state store at {}", path.display()))?;
            tracing::info!(path = %path.display(), "serving read-only views from the state store");
            Some(Arc::new(s))
        }
        None => {
            tracing::info!("no [general] state_path — /v1/portfolio and /v1/activity will 404");
            None
        }
    };

    let state = AppState::new(
        tokens,
        RiskGate::new(cfg.risk.to_core()),
        allowlist,
        cfg.server.to_opts(),
        store,
    );

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
