//! A multi-wallet registry for Robinhood Chain
//! ([ADR-0006](../../../docs/adr/0006-robinhood-chain-venue.md), v0.2.3).
//!
//! Each named [`Wallet`] holds a [`sherwood_signer::LocalSigner`] key, a
//! per-wallet spend ceiling ([`budget::WalletBudget`] — transaction count,
//! cumulative notional, duration), and an optional symbol allowlist ("this
//! wallet only trades these"). Same boundary as `sherwood-signer`, one layer
//! up: **no RPC client, no broadcast method**. A wallet's [`Wallet::signer`]
//! is the only way to reach its key, and that only signs.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod budget;

use budget::{WalletBreach, WalletBudget, WalletBudgetView, WalletLimits};
use rust_decimal::Decimal;
use sherwood_secrets::{resolve_ref, SecretsVault};
use sherwood_signer::LocalSigner;
use std::collections::HashMap;

/// Anything that can go wrong loading the registry.
#[derive(Debug, thiserror::Error)]
pub enum WalletError {
    #[error("duplicate wallet name {0:?}")]
    DuplicateName(String),
    #[error("wallet {name:?}: {source}")]
    Key {
        name: String,
        #[source]
        source: sherwood_signer::SignerError,
    },
    #[error("wallet {name:?}: key_ref {key_ref:?} resolved to no secret — run `sherwood secrets set` first")]
    MissingSecret { name: String, key_ref: String },
    #[error("wallet {name:?}: resolving key_ref: {source}")]
    Vault {
        name: String,
        #[source]
        source: sherwood_secrets::VaultError,
    },
}

/// One wallet's configuration: which key, what it may spend, and what it may
/// trade. Loaded from `config.toml`'s `[[wallets]]` array.
#[derive(Debug, Clone)]
pub struct WalletConfig {
    pub name: String,
    /// A `vault:NAME` reference (or, discouraged, a literal — `resolve_ref`
    /// accepts both, matching the `[ai] api_key` pattern).
    pub key_ref: String,
    /// Symbols this wallet may spend on. Empty = no restriction.
    pub allowed_symbols: Vec<String>,
    pub limits: WalletLimits,
}

/// A named wallet: a key, its spend ceiling, and its symbol allowlist.
pub struct Wallet {
    name: String,
    signer: LocalSigner,
    allowed_symbols: Vec<String>,
    budget: WalletBudget,
}

impl Wallet {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The signer — the only way to reach this wallet's key, and signing is
    /// all it does. Building and broadcasting a transaction from a signature
    /// is `sherwood-dex`'s job, not this crate's.
    #[must_use]
    pub fn signer(&self) -> &LocalSigner {
        &self.signer
    }

    #[must_use]
    pub fn address_hex(&self) -> String {
        self.signer.address_hex()
    }

    /// Whether this wallet is allowed to trade `symbol` (case-insensitive).
    /// An empty allowlist means every symbol is allowed.
    #[must_use]
    pub fn allows_symbol(&self, symbol: &str) -> bool {
        self.allowed_symbols.is_empty()
            || self
                .allowed_symbols
                .iter()
                .any(|s| s.eq_ignore_ascii_case(symbol))
    }

    /// Check and, on success, record a spend of `notional` against this
    /// wallet's budget. Mirrors `sherwood-server`'s per-session budget, one
    /// level down — this does not touch the risk gate or the session budget,
    /// which still apply on top.
    pub fn try_reserve(&self, notional: Decimal) -> Result<(), WalletBreach> {
        self.budget.try_reserve(notional)
    }

    pub fn reset_budget(&self) {
        self.budget.reset();
    }

    #[must_use]
    pub fn budget_view(&self) -> WalletBudgetView {
        self.budget.view()
    }
}

impl std::fmt::Debug for Wallet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wallet")
            .field("name", &self.name)
            .field("address", &self.address_hex())
            .field("allowed_symbols", &self.allowed_symbols)
            .finish_non_exhaustive() // never the key
    }
}

/// Every configured wallet, by name.
#[derive(Debug, Default)]
pub struct WalletRegistry {
    wallets: HashMap<String, Wallet>,
}

impl WalletRegistry {
    /// Load every wallet in `configs`, resolving each `key_ref` against
    /// `vault`. Fails on the first duplicate name, missing secret, or
    /// unparseable key — a wallet registry is all-or-nothing, not
    /// best-effort, since a silently-skipped wallet is a silent capability
    /// loss for whatever strategy expected to use it.
    pub fn load(configs: &[WalletConfig], vault: &dyn SecretsVault) -> Result<Self, WalletError> {
        let mut wallets = HashMap::with_capacity(configs.len());
        for cfg in configs {
            if wallets.contains_key(&cfg.name) {
                return Err(WalletError::DuplicateName(cfg.name.clone()));
            }
            let secret = resolve_ref(&cfg.key_ref, vault)
                .map_err(|source| WalletError::Vault {
                    name: cfg.name.clone(),
                    source,
                })?
                .ok_or_else(|| WalletError::MissingSecret {
                    name: cfg.name.clone(),
                    key_ref: cfg.key_ref.clone(),
                })?;
            let signer = LocalSigner::from_hex(&secret).map_err(|source| WalletError::Key {
                name: cfg.name.clone(),
                source,
            })?;
            wallets.insert(
                cfg.name.clone(),
                Wallet {
                    name: cfg.name.clone(),
                    signer,
                    allowed_symbols: cfg.allowed_symbols.clone(),
                    budget: WalletBudget::new(cfg.limits),
                },
            );
        }
        Ok(Self { wallets })
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Wallet> {
        self.wallets.get(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.wallets.keys().map(String::as_str)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.wallets.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.wallets.is_empty()
    }

    /// The first wallet (by config order isn't preserved by `HashMap`, so
    /// this is "any wallet that allows `symbol`") willing to trade `symbol`
    /// and not currently budget-breached. `None` if no configured wallet
    /// qualifies.
    #[must_use]
    pub fn wallet_for_symbol(&self, symbol: &str) -> Option<&Wallet> {
        self.wallets
            .values()
            .find(|w| w.allows_symbol(symbol) && !w.budget_view().breached)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use sherwood_secrets::VaultError;
    use std::sync::Mutex;

    struct MockVault(Mutex<HashMap<String, String>>);
    impl MockVault {
        fn new(pairs: &[(&str, &str)]) -> Self {
            Self(Mutex::new(
                pairs
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                    .collect(),
            ))
        }
    }
    impl SecretsVault for MockVault {
        fn get(&self, name: &str) -> Result<Option<sherwood_secrets::SecretString>, VaultError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .get(name)
                .map(|v| sherwood_secrets::SecretString::new(v.clone())))
        }
        fn set(&self, name: &str, value: &str) -> Result<(), VaultError> {
            self.0
                .lock()
                .unwrap()
                .insert(name.to_owned(), value.to_owned());
            Ok(())
        }
        fn delete(&self, name: &str) -> Result<bool, VaultError> {
            Ok(self.0.lock().unwrap().remove(name).is_some())
        }
        fn list(&self) -> Result<Vec<String>, VaultError> {
            Ok(self.0.lock().unwrap().keys().cloned().collect())
        }
    }

    fn cfg(name: &str, key_ref: &str) -> WalletConfig {
        WalletConfig {
            name: name.to_owned(),
            key_ref: key_ref.to_owned(),
            allowed_symbols: vec![],
            limits: WalletLimits::default(),
        }
    }

    #[test]
    fn loads_a_wallet_from_a_vault_ref() {
        let vault = MockVault::new(&[("main", &"11".repeat(32))]);
        let reg = WalletRegistry::load(&[cfg("primary", "vault:main")], &vault).unwrap();
        assert_eq!(reg.len(), 1);
        let w = reg.get("primary").unwrap();
        assert_eq!(w.name(), "primary");
        assert!(w.address_hex().starts_with("0x"));
    }

    #[test]
    fn missing_secret_fails_the_whole_load() {
        let vault = MockVault::new(&[]);
        let err = WalletRegistry::load(&[cfg("primary", "vault:main")], &vault).unwrap_err();
        assert!(matches!(err, WalletError::MissingSecret { .. }));
    }

    #[test]
    fn duplicate_names_are_rejected() {
        let vault = MockVault::new(&[("a", &"11".repeat(32)), ("b", &"22".repeat(32))]);
        let configs = vec![cfg("dup", "vault:a"), cfg("dup", "vault:b")];
        let err = WalletRegistry::load(&configs, &vault).unwrap_err();
        assert!(matches!(err, WalletError::DuplicateName(_)));
    }

    #[test]
    fn bad_key_material_fails_the_whole_load() {
        let vault = MockVault::new(&[("main", "not-a-valid-key")]);
        let err = WalletRegistry::load(&[cfg("primary", "vault:main")], &vault).unwrap_err();
        assert!(matches!(err, WalletError::Key { .. }));
    }

    #[test]
    fn debug_never_prints_the_key() {
        let key = "33".repeat(32);
        let vault = MockVault::new(&[("main", &key)]);
        let reg = WalletRegistry::load(&[cfg("primary", "vault:main")], &vault).unwrap();
        let dbg = format!("{:?}", reg.get("primary").unwrap());
        assert!(!dbg.contains(&key));
    }

    #[test]
    fn allows_symbol_is_unrestricted_when_the_allowlist_is_empty() {
        let vault = MockVault::new(&[("main", &"44".repeat(32))]);
        let reg = WalletRegistry::load(&[cfg("primary", "vault:main")], &vault).unwrap();
        let w = reg.get("primary").unwrap();
        assert!(w.allows_symbol("NVDA"));
        assert!(w.allows_symbol("ANYTHING"));
    }

    #[test]
    fn allows_symbol_respects_a_configured_allowlist() {
        let vault = MockVault::new(&[("main", &"55".repeat(32))]);
        let mut c = cfg("primary", "vault:main");
        c.allowed_symbols = vec!["NVDA".into(), "TSLA".into()];
        let reg = WalletRegistry::load(&[c], &vault).unwrap();
        let w = reg.get("primary").unwrap();
        assert!(w.allows_symbol("nvda")); // case-insensitive
        assert!(!w.allows_symbol("AAPL"));
    }

    #[test]
    fn wallet_for_symbol_skips_a_breached_wallet() {
        let vault = MockVault::new(&[("main", &"66".repeat(32))]);
        let mut c = cfg("primary", "vault:main");
        c.limits = WalletLimits {
            max_tx_count: 1,
            ..WalletLimits::default()
        };
        let reg = WalletRegistry::load(&[c], &vault).unwrap();
        let w = reg.get("primary").unwrap();
        assert!(w.try_reserve(dec!(1)).is_ok());
        assert!(w.try_reserve(dec!(1)).is_err()); // now breached
        assert!(reg.wallet_for_symbol("NVDA").is_none());
    }

    #[test]
    fn empty_registry_has_no_wallet_for_any_symbol() {
        let vault = MockVault::new(&[]);
        let reg = WalletRegistry::load(&[], &vault).unwrap();
        assert!(reg.is_empty());
        assert!(reg.wallet_for_symbol("NVDA").is_none());
    }
}
