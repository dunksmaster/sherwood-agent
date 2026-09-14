//! Shared, cheaply-cloneable handler state.

use crate::approvals::{ApprovalMode, ApprovalStore};
use crate::auth::TokenSet;
use crate::budget::{BudgetCaps, SessionBudget};
use crate::limit::RateLimiter;
use crate::metrics::Metrics;
use chrono::{DateTime, Utc};
use sherwood_core::{Fill, RiskGate};
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

/// `POST /v1/dex/simulate` input — the same arguments `sherwood dex-simulate`
/// takes on the command line.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct DexSimulateRequest {
    /// The address to simulate the swap from. Must already hold `token` and
    /// have approved Permit2 for it, or the simulation correctly fails for
    /// lack of balance/allowance — that is not a bug in the request.
    pub from: String,
    /// A known symbol (`"NVDA"`) or a raw token address — the input token.
    pub token: String,
    /// Base units (no decimal scaling) of `token` to swap, as a decimal
    /// string (it can exceed `u64`, e.g. an 18-decimal token).
    pub amount_in_raw: String,
    /// A known symbol or raw address for the output token. Defaults to
    /// `"USDG"` when omitted.
    #[serde(default)]
    pub denom: Option<String>,
    /// Slippage bound in basis points. Defaults to 50 (0.5%) when omitted.
    #[serde(default)]
    pub slippage_bps: Option<u32>,
}

/// `POST /v1/dex/simulate` output: what `eth_call`-simulating the built swap
/// against the live chain found. Signs and sends nothing — same boundary as
/// `sherwood-dex` itself.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DexSimulateOutcome {
    pub token_symbol: String,
    pub denom_symbol: String,
    pub pool_fee: u32,
    pub pool_tick_spacing: i32,
    pub pool_liquidity: String,
    pub amount_out_minimum: String,
    /// `0x`-prefixed calldata this simulation ran — the same bytes a real
    /// swap would sign and send, never done here.
    pub calldata_hex: String,
    /// Whether the `eth_call` simulation succeeded.
    pub ok: bool,
    /// Human-readable detail: bytes returned on success, or why it reverted.
    pub detail: String,
}

/// Runs [`DexSimulateRequest`] against the live chain. Read-only — an
/// `eth_call`, never a send — same boundary as `sherwood-dex`.
/// `sherwood serve` wires a real implementation
/// (`sherwood_cli::dex_preview::ChainDexSimulator`); `None` on [`AppState`]
/// just means the feature isn't configured on this server, not a security
/// gate like [`LivePreflight`].
#[async_trait::async_trait]
pub trait DexSimulator: Send + Sync {
    async fn simulate(&self, req: DexSimulateRequest) -> Result<DexSimulateOutcome, String>;
}

/// `POST /v1/reconcile` input — the same arguments `sherwood reconcile` takes
/// on the command line.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ReconcileRequest {
    /// A transaction hash the operator already obtained by broadcasting
    /// with their own tooling — see
    /// [ADR-0007](../../../docs/adr/0007-no-broadcast-capability.md).
    pub tx_hash: String,
    pub symbol: String,
    /// `"buy"` or `"sell"`.
    pub side: String,
    pub qty: rust_decimal::Decimal,
    pub price: rust_decimal::Decimal,
    #[serde(default)]
    pub fee: Option<rust_decimal::Decimal>,
}

/// What reconciling a transaction hash found. Mirrors
/// `sherwood_reconcile::Outcome`, translated to plain data by whatever
/// implements [`Reconciler`] — this crate does not depend on
/// `sherwood-reconcile` itself, same as [`DexSimulateOutcome`] not
/// depending on `sherwood-dex`.
#[derive(Debug, Clone)]
pub enum ReconcileOutcome {
    /// No receipt yet — still pending, or the hash is unknown to this node.
    NotFound,
    /// Mined, but reverted. Gas was spent; nothing to record.
    Reverted { block_number: u64, gas_used: u64 },
    /// Mined and succeeded. `fill` is ready to record against the
    /// persisted portfolio, exactly like a paper fill.
    Confirmed {
        block_number: u64,
        gas_used: u64,
        fill: Fill,
    },
}

/// Runs [`ReconcileRequest`] against the live chain: reads a receipt only
/// (never sends anything, has no signer) and labels the outcome against the
/// trade the operator expected. `sherwood serve` wires a real
/// implementation (`sherwood_cli::reconcile_preview::ChainReconciler`);
/// `None` on [`AppState`] just means the feature isn't configured on this
/// server.
#[async_trait::async_trait]
pub trait Reconciler: Send + Sync {
    async fn reconcile(&self, req: ReconcileRequest) -> Result<ReconcileOutcome, String>;
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
    /// Venue-selection policy for `POST /v1/route` (`[router]` config).
    /// Cheap and pure — no RPC, no secrets — so it lives directly on
    /// `AppState`, unlike [`DexSimulator`] which needs a chain client.
    pub router_config: sherwood_router::RouterConfig,
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
            router_config: sherwood_router::RouterConfig::default(),
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
    /// Venue-selection policy for `POST /v1/route`. Always present — an
    /// unconfigured `[router]` section is just AMM-only, not a missing
    /// feature.
    pub router_config: sherwood_router::RouterConfig,
    /// Backs `POST /v1/dex/simulate`. `None` = not wired up on this server
    /// (e.g. tests, or a `sherwood serve` invocation with no `[chain]`
    /// endpoint) — the route reports that plainly, not a `500`.
    pub dex_simulator: Option<Arc<dyn DexSimulator>>,
    /// Backs `POST /v1/reconcile`. `None` = not wired up on this server —
    /// the route reports that plainly, not a `500`.
    pub reconciler: Option<Arc<dyn Reconciler>>,
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
            router_config: opts.router_config,
            dex_simulator: None,
            reconciler: None,
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

    #[must_use]
    pub fn with_dex_simulator(mut self, simulator: Arc<dyn DexSimulator>) -> Self {
        self.dex_simulator = Some(simulator);
        self
    }

    #[must_use]
    pub fn with_reconciler(mut self, reconciler: Arc<dyn Reconciler>) -> Self {
        self.reconciler = Some(reconciler);
        self
    }

    pub fn uptime_secs(&self) -> i64 {
        (Utc::now() - self.started_at).num_seconds().max(0)
    }
}
