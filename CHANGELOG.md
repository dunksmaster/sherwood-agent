# Changelog

All notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Until the first `v0.1.0` release the API and schema may change without notice.

## [Unreleased]

### Added
- **`sherwood-events` (S3)** — internal bus (`tokio::sync::broadcast`, bounded 1000)
  behind a `Subscriber` trait. Four event variants, each with a real emitter and
  consumer; every message a versioned `Envelope` (ADR-0004). `TracingSubscriber`
  (always on) and `StoreSubscriber` (persists fills + audit rows). A slow or failing
  subscriber is logged, never fatal. The run loop now publishes events instead of
  calling the store directly.
- **`sherwood-store` (S1)** — SQLite persistence via `sqlx` behind a `Store` trait:
  portfolio snapshots, fill history, and a hash-chained tamper-evident `audit_log`
  with `verify_audit_chain`. Compile-time-checked queries against a committed
  `.sqlx/` offline cache; `SQLX_OFFLINE` in CI so no database is needed.
- `sherwood run` with `[general] state_path` set now persists: it resumes from the
  last portfolio snapshot, records every fill and gate rejection to the audit
  chain, and snapshots on exit (clean interrupt included).
- `Portfolio` is `serde`-serialisable, with a JSON round-trip test.
- Pre-S0 repository hygiene: committed `Cargo.lock`, `deny.toml`, Renovate config,
  workspace lint manifest, `CODEOWNERS`, PR and issue templates, `CLAUDE.md`.
- Repository made public; branch protection enabled on `main`.
- CI expanded: MSRV 1.80 build, `cargo-deny` (licences + RustSec advisories + bans +
  sources), CycloneDX SBOM, `gitleaks`, a coverage report, and a doc-link check.

### Changed
- `PaperExecutor` recovers from a poisoned mutex instead of unwrapping.
- `runner` guards against an empty price series instead of indexing.
- Config validation and graceful shutdown (`docs/CURRENT-STATE.md` defects 9 and 10).

## [0.0.1] - 2026-09-03

The scaffold and the S0 planning documentation. Tagged as the hygiene milestone;
the workspace version remains `0.1.0` (the in-progress target).

### Added
- Rust workspace: `core`, `execution`, `decision`, `copytrade`, `sniper`, `cli`.
- `RiskGate` with eight rejection reasons; `Portfolio` ledger; deterministic
  `PaperExecutor`; `RuleDecider` and an `AiDecider` closure wrapper.
- The S0 documentation set under `docs/`, including three accepted ADRs.

[Unreleased]: https://github.com/dunksmaster/sherwood-agent/compare/v0.0.1...HEAD
[0.0.1]: https://github.com/dunksmaster/sherwood-agent/releases/tag/v0.0.1
