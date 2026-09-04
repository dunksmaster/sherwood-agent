//! `sherwood wallets <config.toml>` — load the `[[wallets]]` registry and
//! print each wallet's name, address, allowed symbols, and budget.
//!
//! Never prints a key. Signs nothing, sends nothing — this only loads and
//! reports; see `sherwood-wallets` and `sherwood-signer`.

use crate::config::AppConfig;
use crate::secrets_cmd::open_vault;
use anyhow::{Context, Result};
use sherwood_wallets::WalletRegistry;

pub fn run(cfg: &AppConfig) -> Result<()> {
    if cfg.wallets.is_empty() {
        println!("no [[wallets]] configured");
        return Ok(());
    }
    let vault = open_vault()?;
    let configs: Vec<_> = cfg
        .wallets
        .iter()
        .map(crate::config::WalletEntry::to_core)
        .collect();
    let registry = WalletRegistry::load(&configs, &vault).context("loading the wallet registry")?;

    let mut names: Vec<&str> = registry.names().collect();
    names.sort_unstable();
    for name in names {
        let Some(w) = registry.get(name) else {
            continue;
        };
        let b = w.budget_view();
        let symbols = if b.tx_cap == 0 && b.notional_cap.is_zero() && b.duration_cap_secs == 0 {
            String::new()
        } else {
            format!(
                "  budget: {}/{} tx, {}/{} notional, {}s/{}s{}",
                b.tx_used,
                b.tx_cap,
                b.notional_used,
                b.notional_cap,
                b.elapsed_secs,
                b.duration_cap_secs,
                if b.breached { " (BREACHED)" } else { "" }
            )
        };
        println!("{name}  {}", w.address_hex());
        let entry = cfg.wallets.iter().find(|e| e.name == name);
        if let Some(entry) = entry {
            if !entry.allowed_symbols.is_empty() {
                println!("  symbols: {}", entry.allowed_symbols.join(", "));
            }
        }
        if !symbols.is_empty() {
            println!("{symbols}");
        }
    }
    Ok(())
}
