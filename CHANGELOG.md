# Changelog

All notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Until the first `v0.1.0` release the API and schema may change without notice.

## [Unreleased]

### Added
- **`sherwood-server` skeleton + `sherwood serve` (S9a)** — the local control-plane HTTP API.
  axum bound to loopback (a non-loopback bind is refused — TLS is a later concern); one
  `{ code, message, correlation_id }` error envelope on every failure; bearer-token auth
  with a constant-time compare (`subtle`), the token generated into the `sherwood-secrets`
  vault on first run and never logged. Routes: `GET /v1/health` (open) and
  `POST /v1/hook/pretooluse` (bearer) which runs S7's `HookGate` against a caller-supplied
  portfolio + market context — a *denied* tool call is a `200` with `{"decision":"deny",…}`,
  only a malformed request is a `4xx`. New `[server]` (bind, `token_ref`) and `[hook]`
  (`read_tools` / `place_tools` / `cancel_tools`) config sections. 13 new tests.
- **`PreToolUse` hook decision core (S7.1–S7.2)** — `sherwood-execution::hook`, the
  fail-closed choke point for [ADR-0001](docs/adr/0001-mcp-interaction-model.md) Option 3.
  `HookGate::evaluate` takes an intercepted agent `ToolCall` and returns `HookOutcome::Allow`
  or `Deny { reason }`. A tool that is not on the `ToolAllowlist` is denied; an order tool
  whose arguments do not parse is denied (never passed through); a parsed order is denied
  unless it passes `RiskGate::check` unchanged; read-only and cancel tools pass without a
  risk check, and cancels pass even when a hard stop is engaged. The allowlist is
  config-driven — no Robinhood tool names are baked in (ADR-0001 open item). `order_parse`
  maps agent tool arguments to a `core::Order` strictly (`symbol` + `side` + `quantity`, or
  `notional` plus a price; `$`/`,` tolerated; floats read via their exact text). The hook's
  HTTP surface and agent-process supervision land with S9. 25 new tests.
- **AI decider (S4)** — `[general] decider = "ai"` drives the paper loop with a language
  model instead of the threshold rules. `AiProvider` trait + `OpenAiCompatProvider`
  (`reqwest` + rustls, whole-request timeout) behind the `decision/openai` feature, working
  against any OpenAI-compatible `/chat/completions` endpoint (NVIDIA NIM, Groq, a local
  server). The `decision` crate owns the prompt: untrusted market data goes in a
  `<market_data>` block, the symbol field is scanned for injection markers, output is
  strict JSON (`deny_unknown_fields`) with a code-fence strip, one retry, then
  degrade-to-`Hold`. Per-run call budget (`ai.max_calls_per_run`) and an optional symbol
  `universe`. The API key is a `vault:` reference resolved at load — a literal key in the
  config is rejected. Every proposal still passes `RiskGate`; the runner stays paper-only.
  15 new tests. See [docs/AI-SAFETY.md](docs/AI-SAFETY.md).
- **`sherwood-secrets` (S6)** — a `FileVault`: Argon2id-derived key, XChaCha20-Poly1305 over a
  JSON `name → value` map, passphrase from `$SHERWOOD_VAULT_PASSPHRASE` (`0600` on Unix).
  `SecretString` zeroes on drop and prints `[redacted]`; `resolve_ref` turns `vault:NAME`
  config references into values. New `sherwood secrets set|get|list|rm` — `set` reads from
  stdin, not argv. 7 tests incl. wrong-passphrase and tamper detection.
- **Multi-asset paper loop + CSV feed (S5.1–5.2)** — `PriceFeed` trait + `Tick` in `core`;
  `CsvFeed` (replays `timestamp,symbol,price` rows) and `SliceFeed` in the CLI. The run loop
  is now driven by a feed, one symbol per tick, with equity and unrealized P&L marked
  against the latest price per held symbol. `[general] feed_path` selects a CSV; `feeds/demo.csv`
  is a runnable sample. The built-in demo feed trades two symbols. Closes CURRENT-STATE
  defects 2 and 3.
- **`RiskGate` extension (S5)** — hard stops (kill switch, realized daily loss) vs entry
  limits. New entry limits, all buy-only so a de-risking sell always passes: an
  **unrealized-loss breaker**, **max concurrent open symbols**, and a **per-symbol buy
  cooldown**. The gate now takes a `GateContext`; `now` is injected via a new
  `sherwood_core::Clock` (`SystemClock` / `FixedClock`). `Portfolio` gains
  `unrealized_pnl` and `open_position_count`. Closes CURRENT-STATE defects 8 and 12.
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
