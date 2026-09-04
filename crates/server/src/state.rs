//! Shared, cheaply-cloneable handler state.

use crate::auth::TokenSet;
use crate::limit::RateLimiter;
use crate::metrics::Metrics;
use chrono::{DateTime, Utc};
use sherwood_core::RiskGate;
use sherwood_execution::ToolAllowlist;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Trading mode. `Live` is reachable only when `allow_live` is set and an admin
/// toggles it with re-auth; the bundled runner is still paper-only, so in v0.1
/// the flag is visible but has no execution path behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Paper,
    Live,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Live => "live",
        }
    }
}

/// The parts of the server that change at runtime. Held behind one `RwLock` so
/// the mode toggle and the kill switch are a single, consistent write.
pub struct Control {
    pub mode: Mode,
    pub risk: RiskGate,
}

impl Control {
    pub fn kill_switch(&self) -> bool {
        self.risk.config().kill_switch
    }
}

/// Knobs that come from `[server]` config.
#[derive(Debug, Clone)]
pub struct ServerOpts {
    /// May an admin switch the mode to `Live` at runtime?
    pub allow_live: bool,
    /// Global request cap per minute (`0` disables limiting).
    pub rate_limit_per_min: u32,
    /// Allowed CORS origins for the dashboard. Empty = no CORS headers.
    pub cors_origins: Vec<String>,
}

impl Default for ServerOpts {
    fn default() -> Self {
        Self {
            allow_live: false,
            rate_limit_per_min: 120,
            cors_origins: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub tokens: Arc<TokenSet>,
    /// Which agent MCP tools may be called, and how each is classified.
    pub allowlist: Arc<ToolAllowlist>,
    pub control: Arc<RwLock<Control>>,
    pub metrics: Arc<Metrics>,
    pub limiter: Arc<RateLimiter>,
    pub allow_live: bool,
    pub cors_origins: Arc<Vec<String>>,
    pub started_at: DateTime<Utc>,
}

impl AppState {
    pub fn new(
        tokens: TokenSet,
        risk: RiskGate,
        allowlist: ToolAllowlist,
        opts: ServerOpts,
    ) -> Self {
        Self {
            tokens: Arc::new(tokens),
            allowlist: Arc::new(allowlist),
            control: Arc::new(RwLock::new(Control {
                mode: Mode::Paper,
                risk,
            })),
            metrics: Arc::new(Metrics::default()),
            limiter: Arc::new(RateLimiter::per_minute(opts.rate_limit_per_min)),
            allow_live: opts.allow_live,
            cors_origins: Arc::new(opts.cors_origins),
            started_at: Utc::now(),
        }
    }

    pub fn uptime_secs(&self) -> i64 {
        (Utc::now() - self.started_at).num_seconds().max(0)
    }
}
