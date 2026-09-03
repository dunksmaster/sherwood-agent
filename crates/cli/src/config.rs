//! TOML config loading. Mirrors `config.example.toml`.

use anyhow::{Context, Result};
use rust_decimal::Decimal;
use serde::Deserialize;
use sherwood_core::RiskConfig;
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub general: General,
    #[serde(default)]
    pub risk: RiskSection,
    #[serde(default)]
    pub copytrade: CopySection,
    #[serde(default)]
    pub sniper: SniperSection,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct General {
    pub starting_cash: Decimal,
    /// Only "paper" is accepted. Any other value is a hard error — the runner
    /// has no live adapter to hand it to.
    pub mode: String,
}

impl Default for General {
    fn default() -> Self {
        Self {
            starting_cash: Decimal::from(1_000),
            mode: "paper".into(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct RiskSection {
    pub max_order_notional: Decimal,
    pub max_position_fraction: Decimal,
    pub max_daily_loss: Decimal,
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
            max_slippage: self.max_slippage,
            allowlist: self.allowlist.clone(),
            denylist: self.denylist.clone(),
            kill_switch: self.kill_switch,
        }
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

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let cfg: AppConfig = toml::from_str(&raw).context("parsing config TOML")?;
        if cfg.general.mode != "paper" {
            anyhow::bail!(
                "general.mode = {:?} is not supported. This runner is paper-only; \
                 build your own binary with a real Executor for live trading.",
                cfg.general.mode
            );
        }
        Ok(cfg)
    }
}
