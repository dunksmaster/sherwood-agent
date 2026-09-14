# sherwood-config

TOML config loading and validation for sherwood-agent (v0.2.12). Mirrors
[`config.example.toml`](../../config.example.toml).

Extracted from `sherwood-cli` so `sherwood-server` can depend on the same
`AppConfig`/`*Section` types directly, for a config-editor endpoint (Phase 3
of the v0.2 remaining-work plan) — without a `server -> config -> server`
cycle.

## What it does

`AppConfig::load(path)` reads and parses a TOML file, then runs
`AppConfig::validate()` — rejecting a config that parses but describes an
impossible or unsafe setup (a non-loopback `server.bind`, a literal API key
where a `vault:` reference is required, an allowlist/denylist overlap, and
so on). Every `*Section` struct derives `Default`, matching the commented-out
defaults in `config.example.toml`.

## Deliberately dependency-light

This crate depends on `sherwood-core`, `sherwood-execution`, and
`sherwood-router` — all three are leaf-ward crates that don't depend on
`sherwood-cli`, `sherwood-server`, or this crate, so none of them create a
cycle. It does **not** depend on `sherwood-server` or `sherwood-wallets`:

- `ServerSection::to_opts() -> sherwood_server::ServerOpts` would recreate
  the exact cycle this extraction exists to break — `sherwood-server` is the
  crate meant to depend on this one.
- `WalletEntry::to_core() -> sherwood_wallets::WalletConfig` is cycle-safe on
  paper (`sherwood-wallets` doesn't depend on `cli`/`server`/`config`
  either), but `sherwood-wallets` pulls in `sherwood-secrets`/
  `sherwood-signer`. Putting the conversion here would transitively hand
  `sherwood-server` a path to vault/signer code, undoing the
  [2026-09-13 decision](../../docs/DECISIONS.md#2026-09-13) to keep that out
  of the network-facing process.

Both conversions live in `sherwood-cli::config_ext` instead — the one crate
meant to depend on both `sherwood-config` and `sherwood-server`/
`sherwood-wallets`. `RiskSection::to_core()`, `HookSection::to_allowlist()`,
and `RouterSection::to_core()`/`validate()` did move here, since their
target crates carry no such risk.

Also **duplicated rather than depended on**: `ChainSection::default()`'s
`rpc_url` is a copy of `sherwood_chain::tokens::DEFAULT_RPC`'s value, not a
dependency on `sherwood-chain` — that crate pulls in `reqwest`/`tokio`,
disproportionate weight for one constant. Keep the two in sync if it changes.

## Not here

- Resolving a `vault:` reference against the actual vault. Every check here
  is a string-prefix check (`starts_with("vault:")`) — no `SecretsVault`
  call anywhere in this crate. Resolution happens at the call site
  (`sherwood-cli`'s `secrets_cmd`/`serve_cmd`).
- `ServerSection::to_opts()` and `WalletEntry::to_core()` — see above.
