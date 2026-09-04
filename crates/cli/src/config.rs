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
    /// If set, `sherwood run` persists to a SQLite database here: the portfolio
    /// snapshot, the fill history, and the tamper-evident audit log. A relative
    /// path is resolved against the working directory. Absent = no persistence.
    #[serde(default)]
    pub state_path: Option<PathBuf>,
}

impl Default for General {
    fn default() -> Self {
        Self {
            starting_cash: Decimal::from(1_000),
            mode: "paper".into(),
            state_path: None,
        }
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

        self.risk.validate()?;

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
            copytrade: CopySection::default(),
            sniper: SniperSection::default(),
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
    fn copytrade_bounds_only_checked_when_leaders_present() {
        let mut c = base();
        c.copytrade.fixed_fraction = Some(dec!(9)); // absurd, but no leaders
        assert!(c.validate().is_ok());
        c.copytrade.leaders.push("0xabc".into());
        assert!(c.validate().is_err());
    }
}
