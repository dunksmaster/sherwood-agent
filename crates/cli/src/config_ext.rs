//! Conversions from `sherwood-config` types into other crates' types.
//!
//! These can't live in `sherwood-config` itself: `ServerSection::to_opts()`
//! returns `sherwood_server::ServerOpts`, and `sherwood-server` is exactly
//! the crate `sherwood-config` exists to be depended on by — moving this in
//! would recreate the cycle the extraction (v0.2.12) was meant to break.
//! `WalletEntry::to_core()` is cycle-safe on paper, but `sherwood-wallets`
//! pulls in `sherwood-secrets`/`sherwood-signer`, and putting it in
//! `sherwood-config` would transitively hand `sherwood-server` a path to
//! vault/signer code — undoing the 2026-09-13 decision to keep that out of
//! the network-facing process. `sherwood-cli` is the only crate meant to
//! depend on both `sherwood-config` and `sherwood-server`/`sherwood-wallets`,
//! so this is where both conversions live.

use crate::config::{ServerSection, WalletEntry};

pub trait ServerSectionExt {
    /// The runtime knobs `sherwood-server` needs. `AppConfig::validate()` has
    /// already checked `approval_mode`, so the fallback here is unreachable.
    fn to_opts(&self) -> sherwood_server::ServerOpts;
}

impl ServerSectionExt for ServerSection {
    fn to_opts(&self) -> sherwood_server::ServerOpts {
        sherwood_server::ServerOpts {
            allow_live: self.allow_live,
            rate_limit_per_min: self.rate_limit_per_min,
            cors_origins: self.cors_origins.clone(),
            static_dir: self.static_dir.clone(),
            approval_mode: sherwood_server::approvals::ApprovalMode::parse(&self.approval_mode)
                .unwrap_or(sherwood_server::approvals::ApprovalMode::Auto),
            approval_timeout: std::time::Duration::from_secs(self.approval_timeout_secs.max(1)),
            budget_caps: sherwood_server::budget::BudgetCaps {
                max_orders: self.max_session_orders,
                max_notional: self.max_session_notional,
                max_duration: std::time::Duration::from_secs(self.max_session_duration_secs),
            },
            // `[router]` lives in a different config section; callers
            // overwrite this with `cfg.router.to_core()` after calling
            // `to_opts()`. AMM-only here is the same default `[router]`
            // itself falls back to.
            router_config: sherwood_router::RouterConfig::default(),
        }
    }
}

pub trait WalletEntryExt {
    fn to_core(&self) -> sherwood_wallets::WalletConfig;
}

impl WalletEntryExt for WalletEntry {
    fn to_core(&self) -> sherwood_wallets::WalletConfig {
        sherwood_wallets::WalletConfig {
            name: self.name.clone(),
            key_ref: self.key_ref.clone(),
            allowed_symbols: self.allowed_symbols.clone(),
            limits: sherwood_wallets::budget::WalletLimits {
                max_tx_count: self.max_tx_count,
                max_notional: self.max_notional,
                max_duration: std::time::Duration::from_secs(self.max_duration_secs),
            },
        }
    }
}
