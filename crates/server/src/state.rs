//! Shared, cheaply-cloneable handler state.

use crate::approvals::{ApprovalMode, ApprovalStore};
use crate::auth::TokenSet;
use crate::budget::{BudgetCaps, SessionBudget};
use crate::limit::RateLimiter;
use crate::metrics::Metrics;
use chrono::{DateTime, Utc};
use sherwood_core::RiskGate;
use sherwood_execution::ToolAllowlist;
use sherwood_store::SqliteStore;
use std::sync::Arc;
use std::time::Duration;
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

/// The parts of the server that change at runtime — the mode toggle, the kill
/// switch, and a `POST /v1/config/reload`. Held behind one `RwLock` so each is
/// a single consistent write.
pub struct Control {
    pub mode: Mode,
    pub risk: RiskGate,
    /// Which agent MCP tools may be called, and how each is classified.
    pub allowlist: ToolAllowlist,
    /// `Auto` = the approval gate is transparent; `Manual` = every risk-passing
    /// order waits for the operator.
    pub approval_mode: ApprovalMode,
}

impl Control {
    pub fn kill_switch(&self) -> bool {
        self.risk.config().kill_switch
    }
}

/// The subset of config a `POST /v1/config/reload` may swap in without a
/// restart. Built by the CLI from a re-read, re-validated `config.toml`.
pub struct Reloaded {
    pub risk: RiskGate,
    pub allowlist: ToolAllowlist,
    pub approval_mode: ApprovalMode,
}

/// Re-reads and re-validates the config file, or returns why it could not.
pub type Reloader = Arc<dyn Fn() -> Result<Reloaded, String> + Send + Sync>;

/// The [ADR-0006](../../../docs/adr/0006-robinhood-chain-venue.md) mandatory
/// pre-flight for arming live mode: is a fresh, un-onboarded address still
/// able to receive every live-tradeable token permissionlessly — the same
/// check `sherwood chain-probe` runs by hand. `Ok(())` means it is safe to
/// arm; `Err(reason)` refuses, and the reason is returned to the caller and
/// logged.
///
/// Live mode has no order-placing path in this codebase yet, but arming is
/// gated regardless: the flag itself must not flip on a chain whose transfer
/// semantics have changed since ADR-0006 was decided (e.g. an implementation
/// upgrade adding an allowlist) — checking only when an order is placed would
/// be too late for an operator who armed live mode and only found out at the
/// next tick.
#[async_trait::async_trait]
pub trait LivePreflight: Send + Sync {
    async fn check(&self) -> Result<(), String>;
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
    /// Directory of the built dashboard (`frontend/dist`) to serve at `/`.
    /// `None` = API only.
    pub static_dir: Option<std::path::PathBuf>,
    /// `Auto` = the approval gate is transparent; `Manual` = every risk-passing
    /// order waits for the operator.
    pub approval_mode: ApprovalMode,
    /// How long a pending approval waits before it auto-denies.
    pub approval_timeout: Duration,
    /// Per-session spend caps (order count / notional / duration). Any `0` is
    /// "no limit".
    pub budget_caps: BudgetCaps,
}

impl Default for ServerOpts {
    fn default() -> Self {
        Self {
            allow_live: false,
            rate_limit_per_min: 120,
            cors_origins: Vec::new(),
            static_dir: None,
            approval_mode: ApprovalMode::Auto,
            approval_timeout: Duration::from_secs(60),
            budget_caps: BudgetCaps::default(),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub tokens: Arc<TokenSet>,
    pub control: Arc<RwLock<Control>>,
    pub metrics: Arc<Metrics>,
    pub limiter: Arc<RateLimiter>,
    /// Read-only handle to the persisted state written by `sherwood run`.
    /// `None` when no `state_path` is configured.
    pub store: Option<Arc<SqliteStore>>,
    pub allow_live: bool,
    pub cors_origins: Arc<Vec<String>>,
    /// Built dashboard directory, if the server should serve it.
    pub static_dir: Option<Arc<std::path::PathBuf>>,
    pub approvals: Arc<ApprovalStore>,
    pub budget: Arc<SessionBudget>,
    /// Re-reads `config.toml` for `POST /v1/config/reload`. `None` = reload is
    /// unavailable (e.g. tests).
    pub reloader: Option<Reloader>,
    /// The ADR-0006 pre-flight `POST /v1/mode` must pass before arming
    /// `Live`. `None` means no pre-flight is wired up — live mode then stays
    /// unreachable (fail closed) regardless of `allow_live`.
    pub live_preflight: Option<Arc<dyn LivePreflight>>,
    pub started_at: DateTime<Utc>,
}

impl AppState {
    pub fn new(
        tokens: TokenSet,
        risk: RiskGate,
        allowlist: ToolAllowlist,
        opts: ServerOpts,
        store: Option<Arc<SqliteStore>>,
    ) -> Self {
        Self {
            tokens: Arc::new(tokens),
            control: Arc::new(RwLock::new(Control {
                mode: Mode::Paper,
                risk,
                allowlist,
                approval_mode: opts.approval_mode,
            })),
            metrics: Arc::new(Metrics::default()),
            limiter: Arc::new(RateLimiter::per_minute(opts.rate_limit_per_min)),
            store,
            allow_live: opts.allow_live,
            cors_origins: Arc::new(opts.cors_origins),
            static_dir: opts.static_dir.map(Arc::new),
            approvals: Arc::new(ApprovalStore::new(opts.approval_timeout)),
            budget: Arc::new(SessionBudget::new(opts.budget_caps)),
            reloader: None,
            live_preflight: None,
            started_at: Utc::now(),
        }
    }

    #[must_use]
    pub fn with_reloader(mut self, reloader: Reloader) -> Self {
        self.reloader = Some(reloader);
        self
    }

    #[must_use]
    pub fn with_live_preflight(mut self, preflight: Arc<dyn LivePreflight>) -> Self {
        self.live_preflight = Some(preflight);
        self
    }

    pub fn uptime_secs(&self) -> i64 {
        (Utc::now() - self.started_at).num_seconds().max(0)
    }
}
