//! TOML config loading. Mirrors `config.example.toml`.

use anyhow::{bail, Context, Result};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Deserialize;
use sherwood_core::RiskConfig;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Require `value` to lie in `(low, high]`. `name` is used in the error.
fn require_in_half_open(name: &str, value: Decimal, low: Decimal, high: Decimal) -> Result<()> {
    if value <= low || value > high {
        bail!("{name} must be in ({low}, {high}] — got {value}");
    }
    Ok(())
}

/// Require `value` to lie in `[low, high]`.
fn require_in_closed(name: &str, value: Decimal, low: Decimal, high: Decimal) -> Result<()> {
    if value < low || value > high {
        bail!("{name} must be in [{low}, {high}] — got {value}");
    }
    Ok(())
}

/// Require `value >= low`.
fn require_at_least(name: &str, value: Decimal, low: Decimal) -> Result<()> {
    if value < low {
        bail!("{name} must be >= {low} — got {value}");
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub general: General,
    #[serde(default)]
    pub risk: RiskSection,
    #[serde(default)]
    pub ai: AiSection,
    #[serde(default)]
    pub copytrade: CopySection,
    #[serde(default)]
    pub sniper: SniperSection,
    #[serde(default)]
    pub server: ServerSection,
    #[serde(default)]
    pub hook: HookSection,
    #[serde(default)]
    pub chain: ChainSection,
    #[serde(default)]
    pub wallets: Vec<WalletEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct General {
    pub starting_cash: Decimal,
    /// Only "paper" is accepted. Any other value is a hard error — the runner
    /// has no live adapter to hand it to.
    pub mode: String,
    /// If set, `sherwood run` persists to a SQLite database here: the portfolio
    /// snapshot, the fill history, and the tamper-evident audit log. A relative
    /// path is resolved against the working directory. Absent = no persistence.
    #[serde(default)]
    pub state_path: Option<PathBuf>,
    /// CSV price feed to replay (`timestamp,symbol,price` rows). Absent = the
    /// built-in two-symbol demo feed. Ignored if `[chain] enabled = true` — the
    /// chain feed takes precedence.
    #[serde(default)]
    pub feed_path: Option<PathBuf>,
    /// Which decider drives entries: `"rule"` (deterministic thresholds, the
    /// default) or `"ai"` (a language model via the `[ai]` section). Either way
    /// the output is advisory — `RiskGate` still has the final say.
    pub decider: String,
}

impl Default for General {
    fn default() -> Self {
        Self {
            starting_cash: Decimal::from(1_000),
            mode: "paper".into(),
            state_path: None,
            feed_path: None,
            decider: "rule".into(),
        }
    }
}

/// A live price feed off Robinhood Chain instead of a CSV replay. **Still
/// paper trading** — this only supplies prices; no wallet, no signing, no
/// order ever reaches the venue. See [ADR-0006](../../../docs/adr/0006-robinhood-chain-venue.md)
/// and `sherwood_chain::feed::ChainFeed`.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct ChainSection {
    pub enabled: bool,
    /// JSON-RPC endpoint. Defaults to Robinhood Chain's public mainnet RPC.
    pub rpc_url: String,
    /// Known symbols (`"NVDA"`) or raw addresses to poll, round-robin.
    pub symbols: Vec<String>,
    /// The token every price is quoted in — a known symbol or address.
    pub denom: String,
    /// Seconds between completing one round of all `symbols` and starting the
    /// next.
    pub poll_interval_secs: u64,
}

impl Default for ChainSection {
    fn default() -> Self {
        Self {
            enabled: false,
            rpc_url: sherwood_chain::tokens::DEFAULT_RPC.to_owned(),
            symbols: Vec::new(),
            denom: "USDG".into(),
            poll_interval_secs: 15,
        }
    }
}

impl ChainSection {
    fn validate(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        if self.symbols.is_empty() {
            bail!("chain.enabled = true needs at least one symbol in chain.symbols");
        }
        if self.rpc_url.is_empty() {
            bail!("chain.rpc_url must not be empty");
        }
        if self.poll_interval_secs == 0 {
            bail!("chain.poll_interval_secs must be > 0");
        }
        Ok(())
    }
}

/// The language-model decider. Only consulted when `general.decider = "ai"`.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct AiSection {
    /// OpenAI-compatible API root, no trailing slash — e.g.
    /// `https://integrate.api.nvidia.com/v1` (NVIDIA NIM) or a Groq / local
    /// endpoint.
    pub base_url: String,
    /// Model identifier the endpoint expects.
    pub model: String,
    /// A vault reference — `"vault:nvidia"`. A literal key here is rejected;
    /// secrets never live in a config file.
    pub api_key: String,
    /// Sampling temperature. Low is appropriate for a decision task.
    pub temperature: f64,
    /// `max_tokens` on each completion request.
    pub max_tokens: u32,
    /// Stop calling the provider after this many calls in one run (`0` = no
    /// limit). Once tripped, every decision is `Hold`.
    pub max_calls_per_run: u32,
    /// Whole-round-trip timeout for one completion. On expiry the decider holds.
    pub request_timeout_secs: u64,
    /// If non-empty, the model may only name these symbols; anything else holds.
    pub universe: Vec<String>,
}

impl Default for AiSection {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            model: String::new(),
            api_key: String::new(),
            temperature: 0.2,
            max_tokens: 300,
            max_calls_per_run: 50,
            request_timeout_secs: 20,
            universe: Vec::new(),
        }
    }
}

impl AiSection {
    fn validate(&self) -> Result<()> {
        for (name, v) in [
            ("ai.base_url", &self.base_url),
            ("ai.model", &self.model),
            ("ai.api_key", &self.api_key),
        ] {
            if v.trim().is_empty() {
                bail!("{name} is required when general.decider = \"ai\"");
            }
        }
        if !self.base_url.starts_with("http://") && !self.base_url.starts_with("https://") {
            bail!(
                "ai.base_url must be an http(s) URL — got {:?}",
                self.base_url
            );
        }
        if !self.api_key.starts_with("vault:") {
            bail!(
                "ai.api_key must be a vault reference like \"vault:nvidia\", never a literal \
                 key — store the key with `sherwood secrets set`"
            );
        }
        require_in_closed(
            "ai.temperature",
            Decimal::try_from(self.temperature).unwrap_or(dec!(0)),
            dec!(0),
            dec!(2),
        )?;
        if self.max_tokens == 0 {
            bail!("ai.max_tokens must be at least 1");
        }
        if self.request_timeout_secs == 0 {
            bail!("ai.request_timeout_secs must be at least 1");
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct RiskSection {
    pub max_order_notional: Decimal,
    pub max_position_fraction: Decimal,
    pub max_daily_loss: Decimal,
    pub max_unrealized_loss: Decimal,
    pub max_open_positions: usize,
    pub order_cooldown_secs: u64,
    pub max_slippage: Decimal,
    pub allowlist: HashSet<String>,
    pub denylist: HashSet<String>,
    pub kill_switch: bool,
}

impl Default for RiskSection {
    fn default() -> Self {
        let d = RiskConfig::default();
        Self {
            max_order_notional: d.max_order_notional,
            max_position_fraction: d.max_position_fraction,
            max_daily_loss: d.max_daily_loss,
            max_unrealized_loss: d.max_unrealized_loss,
            max_open_positions: d.max_open_positions,
            order_cooldown_secs: d.order_cooldown_secs,
            max_slippage: d.max_slippage,
            allowlist: d.allowlist,
            denylist: d.denylist,
            kill_switch: d.kill_switch,
        }
    }
}

impl RiskSection {
    pub fn to_core(&self) -> RiskConfig {
        RiskConfig {
            max_order_notional: self.max_order_notional,
            max_position_fraction: self.max_position_fraction,
            max_daily_loss: self.max_daily_loss,
            max_unrealized_loss: self.max_unrealized_loss,
            max_open_positions: self.max_open_positions,
            order_cooldown_secs: self.order_cooldown_secs,
            max_slippage: self.max_slippage,
            allowlist: self.allowlist.clone(),
            denylist: self.denylist.clone(),
            kill_switch: self.kill_switch,
        }
    }

    fn validate(&self) -> Result<()> {
        require_in_half_open(
            "risk.max_order_notional",
            self.max_order_notional,
            dec!(0),
            Decimal::MAX,
        )?;
        require_in_half_open(
            "risk.max_position_fraction",
            self.max_position_fraction,
            dec!(0),
            dec!(1),
        )?;
        require_at_least("risk.max_daily_loss", self.max_daily_loss, dec!(0))?;
        require_at_least(
            "risk.max_unrealized_loss",
            self.max_unrealized_loss,
            dec!(0),
        )?;
        if self.max_open_positions == 0 {
            bail!("risk.max_open_positions must be at least 1");
        }
        require_in_closed("risk.max_slippage", self.max_slippage, dec!(0), dec!(1))?;

        let overlap: Vec<&String> = self.allowlist.intersection(&self.denylist).collect();
        if !overlap.is_empty() {
            bail!("risk.allowlist and risk.denylist share symbols: {overlap:?}");
        }
        Ok(())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct CopySection {
    pub leaders: Vec<String>,
    pub fixed_fraction: Option<Decimal>,
    pub min_leader_notional: Option<Decimal>,
    pub max_mirror_notional: Option<Decimal>,
    pub slippage: Option<Decimal>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct SniperSection {
    pub enabled: bool,
    pub min_initial_liquidity: Option<Decimal>,
    pub entry_notional: Option<Decimal>,
}

/// `sherwood serve` — the local control-plane HTTP API.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct ServerSection {
    /// `ip:port` to bind. Loopback only — a public bind needs TLS and is
    /// refused (see `docs/SECURITY.md`).
    pub bind: String,
    /// Where the admin bearer token lives. A `vault:` reference; generated into
    /// the vault on first `serve` if absent.
    pub token_ref: String,
    /// Optional `operator`-role token reference. Absent = no operator token.
    pub operator_token_ref: Option<String>,
    /// Optional `viewer`-role token reference. Absent = no viewer token.
    pub viewer_token_ref: Option<String>,
    /// Whether an admin may switch the mode to LIVE at runtime. The bundled
    /// runner is paper-only regardless; this just gates the toggle.
    pub allow_live: bool,
    /// Global request cap per minute (`0` disables it).
    pub rate_limit_per_min: u32,
    /// Browser origins allowed to call the API (the dashboard dev server, the
    /// served origin). Empty = same-origin only, no CORS headers.
    pub cors_origins: Vec<String>,
    /// Directory of the built dashboard (`frontend/dist`) to serve at `/`.
    /// Absent = API only.
    pub static_dir: Option<PathBuf>,
    /// `"auto"` = the approval gate is transparent (the risk gate decides).
    /// `"manual"` = every risk-passing order waits for the operator.
    pub approval_mode: String,
    /// Seconds a pending approval waits before it auto-denies.
    pub approval_timeout_secs: u64,
    /// Per-session hard stops. Any `0` means "no limit". Once tripped, every
    /// further order is denied until `POST /v1/session/reset`.
    pub max_session_orders: u32,
    pub max_session_notional: Decimal,
    pub max_session_duration_secs: u64,
}

impl Default for ServerSection {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8787".into(),
            token_ref: "vault:api_token".into(),
            operator_token_ref: None,
            viewer_token_ref: None,
            allow_live: false,
            rate_limit_per_min: 120,
            cors_origins: Vec::new(),
            static_dir: None,
            approval_mode: "auto".into(),
            approval_timeout_secs: 60,
            max_session_orders: 0,
            max_session_notional: dec!(0),
            max_session_duration_secs: 0,
        }
    }
}

impl ServerSection {
    /// The runtime knobs `sherwood-server` needs. `validate()` has already
    /// checked `approval_mode`, so the fallback here is unreachable.
    pub fn to_opts(&self) -> sherwood_server::ServerOpts {
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
        }
    }

    fn validate(&self) -> Result<()> {
        if let Some(dir) = &self.static_dir {
            if !dir.join("index.html").is_file() {
                bail!(
                    "server.static_dir {:?} has no index.html — point it at `frontend/dist` \
                     after `npm run build`",
                    dir
                );
            }
        }
        if !matches!(self.approval_mode.as_str(), "auto" | "manual") {
            bail!(
                "server.approval_mode must be \"auto\" or \"manual\", got {:?}",
                self.approval_mode
            );
        }
        let addr: std::net::SocketAddr = self
            .bind
            .parse()
            .with_context(|| format!("server.bind {:?} is not a valid ip:port", self.bind))?;
        if !addr.ip().is_loopback() {
            bail!("server.bind {addr} is not loopback; a public bind needs TLS and is refused");
        }
        for (name, r) in [
            ("server.token_ref", Some(&self.token_ref)),
            (
                "server.operator_token_ref",
                self.operator_token_ref.as_ref(),
            ),
            ("server.viewer_token_ref", self.viewer_token_ref.as_ref()),
        ] {
            if let Some(r) = r {
                if !r.starts_with("vault:") {
                    bail!("{name} must be a vault reference like \"vault:api_token\"");
                }
            }
        }
        Ok(())
    }
}

/// Which agent MCP tools the `PreToolUse` hook permits, and how each is
/// classified. Empty lists mean "deny everything" — the safe default.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct HookSection {
    pub read_tools: Vec<String>,
    pub place_tools: Vec<String>,
    pub cancel_tools: Vec<String>,
}

impl HookSection {
    pub fn to_allowlist(&self) -> sherwood_execution::ToolAllowlist {
        use sherwood_execution::ToolClass;
        let mut a = sherwood_execution::ToolAllowlist::new();
        for t in &self.read_tools {
            a.allow(t, ToolClass::ReadOnly);
        }
        for t in &self.place_tools {
            a.allow(t, ToolClass::PlaceOrder);
        }
        for t in &self.cancel_tools {
            a.allow(t, ToolClass::CancelOrder);
        }
        a
    }
}

/// One `[[wallets]]` entry: a name, which vault key it signs with, what it
/// may trade, and its spend ceiling. See `sherwood-wallets`. Loading these
/// (`sherwood wallets`) resolves each `key_ref` — no wallet is a silent
/// no-op if its secret is missing.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WalletEntry {
    pub name: String,
    /// A `vault:NAME` reference — same pattern as `[ai] api_key`.
    pub key_ref: String,
    /// Symbols this wallet may trade. Empty = no restriction.
    pub allowed_symbols: Vec<String>,
    /// Any `0` = unlimited.
    pub max_tx_count: u32,
    pub max_notional: Decimal,
    pub max_duration_secs: u64,
}

impl Default for WalletEntry {
    fn default() -> Self {
        Self {
            name: String::new(),
            key_ref: String::new(),
            allowed_symbols: Vec::new(),
            max_tx_count: 0,
            max_notional: Decimal::ZERO,
            max_duration_secs: 0,
        }
    }
}

impl WalletEntry {
    fn validate(&self) -> Result<()> {
        if self.name.is_empty() {
            bail!("every [[wallets]] entry needs a name");
        }
        if self.key_ref.is_empty() {
            bail!("wallets.{}: key_ref must not be empty", self.name);
        }
        require_at_least(
            &format!("wallets.{}.max_notional", self.name),
            self.max_notional,
            dec!(0),
        )?;
        Ok(())
    }

    #[must_use]
    pub fn to_core(&self) -> sherwood_wallets::WalletConfig {
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

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let cfg: AppConfig = toml::from_str(&raw).context("parsing config TOML")?;
        cfg.validate()
            .with_context(|| format!("invalid config {}", path.display()))?;
        Ok(cfg)
    }

    /// Reject a config that parses but describes an impossible or unsafe setup.
    /// Runs after deserialisation, on every load.
    pub fn validate(&self) -> Result<()> {
        if self.general.mode != "paper" {
            bail!(
                "general.mode = {:?} is not supported. This runner is paper-only; \
                 build your own binary with a real Executor for live trading.",
                self.general.mode
            );
        }
        require_in_half_open(
            "general.starting_cash",
            self.general.starting_cash,
            dec!(0),
            Decimal::MAX,
        )?;

        match self.general.decider.as_str() {
            "rule" => {}
            "ai" => self.ai.validate()?,
            other => bail!("general.decider = {other:?} is not \"rule\" or \"ai\""),
        }

        self.risk.validate()?;
        self.server.validate()?;
        self.chain.validate()?;
        for w in &self.wallets {
            w.validate()?;
        }
        {
            let mut names: Vec<&str> = self.wallets.iter().map(|w| w.name.as_str()).collect();
            names.sort_unstable();
            if names.windows(2).any(|pair| pair[0] == pair[1]) {
                bail!("duplicate wallet name in [[wallets]]");
            }
        }

        if !self.copytrade.leaders.is_empty() {
            if let Some(f) = self.copytrade.fixed_fraction {
                require_in_half_open("copytrade.fixed_fraction", f, dec!(0), dec!(1))?;
            }
            for (name, v) in [
                (
                    "copytrade.min_leader_notional",
                    self.copytrade.min_leader_notional,
                ),
                (
                    "copytrade.max_mirror_notional",
                    self.copytrade.max_mirror_notional,
                ),
            ] {
                if let Some(v) = v {
                    require_at_least(name, v, dec!(0))?;
                }
            }
            if let Some(s) = self.copytrade.slippage {
                require_in_closed("copytrade.slippage", s, dec!(0), dec!(1))?;
            }
        }

        if self.sniper.enabled {
            for (name, v) in [
                (
                    "sniper.min_initial_liquidity",
                    self.sniper.min_initial_liquidity,
                ),
                ("sniper.entry_notional", self.sniper.entry_notional),
            ] {
                if let Some(v) = v {
                    require_in_half_open(name, v, dec!(0), Decimal::MAX)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> AppConfig {
        AppConfig {
            general: General::default(),
            risk: RiskSection::default(),
            ai: AiSection::default(),
            copytrade: CopySection::default(),
            sniper: SniperSection::default(),
            server: ServerSection::default(),
            hook: HookSection::default(),
            chain: ChainSection::default(),
            wallets: Vec::new(),
        }
    }

    #[test]
    fn default_config_is_valid() {
        assert!(base().validate().is_ok());
    }

    #[test]
    fn rejects_position_fraction_above_one() {
        let mut c = base();
        c.risk.max_position_fraction = dec!(1.5);
        let err = c.validate().unwrap_err().to_string();
        assert!(err.contains("max_position_fraction"), "{err}");
    }

    #[test]
    fn rejects_negative_position_fraction() {
        let mut c = base();
        c.risk.max_position_fraction = dec!(-0.1);
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_zero_starting_cash() {
        let mut c = base();
        c.general.starting_cash = dec!(0);
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_allow_deny_overlap() {
        let mut c = base();
        c.risk.allowlist.insert("ROAR".into());
        c.risk.denylist.insert("ROAR".into());
        let err = c.validate().unwrap_err().to_string();
        assert!(err.contains("share symbols"), "{err}");
    }

    #[test]
    fn rejects_non_paper_mode() {
        let mut c = base();
        c.general.mode = "live".into();
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_a_non_loopback_server_bind() {
        let mut c = base();
        c.server.bind = "0.0.0.0:8787".into();
        let err = c.validate().unwrap_err().to_string();
        assert!(err.contains("loopback"), "{err}");
    }

    #[test]
    fn rejects_a_garbage_server_bind() {
        let mut c = base();
        c.server.bind = "not-an-addr".into();
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_an_unknown_approval_mode() {
        let mut c = base();
        c.server.approval_mode = "yolo".into();
        let err = c.validate().unwrap_err().to_string();
        assert!(err.contains("approval_mode"), "{err}");
        c.server.approval_mode = "manual".into();
        assert!(c.validate().is_ok());
    }

    #[test]
    fn role_token_refs_must_be_vault_references() {
        let mut c = base();
        c.server.operator_token_ref = Some("literal-token".into());
        let err = c.validate().unwrap_err().to_string();
        assert!(err.contains("vault reference"), "{err}");
        c.server.operator_token_ref = Some("vault:api_token_operator".into());
        assert!(c.validate().is_ok());
    }

    #[test]
    fn hook_section_maps_to_a_classified_allowlist() {
        use sherwood_execution::ToolClass;
        let mut c = base();
        c.hook.read_tools = vec!["get_positions".into()];
        c.hook.place_tools = vec!["place_order".into()];
        c.hook.cancel_tools = vec!["cancel_order".into()];
        let al = c.hook.to_allowlist();
        assert_eq!(al.classify("get_positions"), Some(ToolClass::ReadOnly));
        assert_eq!(al.classify("place_order"), Some(ToolClass::PlaceOrder));
        assert_eq!(al.classify("cancel_order"), Some(ToolClass::CancelOrder));
        assert_eq!(al.classify("something_else"), None);
    }

    #[test]
    fn rejects_unknown_decider() {
        let mut c = base();
        c.general.decider = "magic".into();
        let err = c.validate().unwrap_err().to_string();
        assert!(err.contains("decider"), "{err}");
    }

    #[test]
    fn ai_section_ignored_unless_decider_is_ai() {
        let mut c = base();
        c.ai.base_url = "not-a-url".into(); // nonsense, but decider is "rule"
        assert!(c.validate().is_ok());
    }

    #[test]
    fn ai_decider_requires_a_configured_section() {
        let mut c = base();
        c.general.decider = "ai".into();
        assert!(c.validate().is_err()); // base_url/model/api_key all empty
    }

    #[test]
    fn ai_api_key_must_be_a_vault_reference() {
        let mut c = base();
        c.general.decider = "ai".into();
        c.ai.base_url = "https://integrate.api.nvidia.com/v1".into();
        c.ai.model = "meta/llama-3.1-70b-instruct".into();
        c.ai.api_key = "nvapi-literal-key".into();
        let err = c.validate().unwrap_err().to_string();
        assert!(err.contains("vault reference"), "{err}");

        c.ai.api_key = "vault:nvidia".into();
        assert!(c.validate().is_ok());
    }

    #[test]
    fn copytrade_bounds_only_checked_when_leaders_present() {
        let mut c = base();
        c.copytrade.fixed_fraction = Some(dec!(9)); // absurd, but no leaders
        assert!(c.validate().is_ok());
        c.copytrade.leaders.push("0xabc".into());
        assert!(c.validate().is_err());
    }

    #[test]
    fn chain_feed_disabled_by_default_needs_no_symbols() {
        let c = base();
        assert!(!c.chain.enabled);
        assert!(c.validate().is_ok());
    }

    #[test]
    fn enabling_the_chain_feed_without_symbols_is_rejected() {
        let mut c = base();
        c.chain.enabled = true;
        assert!(c.validate().is_err());
        c.chain.symbols.push("NVDA".into());
        assert!(c.validate().is_ok());
    }

    #[test]
    fn chain_feed_rejects_a_zero_poll_interval() {
        let mut c = base();
        c.chain.enabled = true;
        c.chain.symbols.push("NVDA".into());
        c.chain.poll_interval_secs = 0;
        assert!(c.validate().is_err());
    }
}
