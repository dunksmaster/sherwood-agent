# Changelog

All notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Until the first `v0.1.0` release the API and schema may change without notice.

## [Unreleased]

### Added
- **Observability (S13a)** — `GET /v1/metrics` gains `sherwood_kill_switch`,
  `sherwood_mode_live`, `sherwood_approvals_pending`, `sherwood_session_orders_used`,
  `sherwood_session_notional_used`, and `sherwood_session_budget_breached` gauges — enough
  for real alerting. `$SHERWOOD_LOG_DIR` turns on a daily-rolling JSON log file
  (`tracing-appender`) alongside the console. New [`deploy/`](deploy/README.md): a Prometheus
  scrape config, alert rules (server down, kill switch, budget breached, approvals backlog,
  LIVE mode, high 5xx), and an importable Grafana dashboard. `docs/OBSERVABILITY.md` written.
- **`sherwood backtest` (S14a)** — replays the `[general] feed_path` CSV through the
  configured decider and the risk gate (the same `run_loop` as `sherwood run`, same paper
  executor) and prints a performance summary: total return, max drawdown (peak-to-trough of
  the per-tick equity curve), fills, closed trades + win rate, gross profit / loss, profit
  factor, expectancy per trade. Closed-trade P&L uses average-cost basis; a partial sell
  closes a proportional slice. Deterministic, nothing persisted, `order_cooldown_secs` forced
  to `0`. See [docs/BACKTEST.md](docs/BACKTEST.md) for what it does and does **not** tell you.
- **Per-session spend budgets (S12a)** — three `[server]` hard stops independent of the risk
  config: `max_session_orders`, `max_session_notional`, `max_session_duration_secs` (any `0`
  = unlimited). Once a cap latches, `POST /v1/hook/pretooluse` denies every further
  place-order (`"session budget: …"`) until an admin calls `POST /v1/session/reset` (with
  body re-auth). Reads and cancels never touch the budget. `GET /v1/session` shows
  `{ orders_used/cap, notional_used/cap, elapsed_secs/cap, breached }`; the dashboard shows a
  budget line with a Reset button. The cron scheduler and price-threshold monitors (the rest
  of S12) are deferred — they drive the run loop, which `serve` does not host.
- **Approval gate (S11)** — a human-in-the-loop step between the risk gate and the venue
  ([ADR-0005](docs/adr/0005-approval-gate.md)). `[server] approval_mode`: `auto` (default,
  transparent — unchanged) or `manual`. In `manual` mode a risk-passing **place-order** call
  creates a `pending` approval and `POST /v1/hook/pretooluse` holds its response until the
  operator approves or denies it, or `approval_timeout_secs` (default 60) elapses and it
  auto-denies. Reads and cancels are never held. New routes: `GET /v1/approvals`
  (`{ mode, pending, approvals[] }`, viewer) and `POST /v1/approvals/{id}`
  (`{ decision: "approve" | "deny", reason? }`, operator). A 5s sweeper expires stale
  pendings even with no hook waiting. In-memory, capped history; a restart denies anything
  pending. Dashboard gains an `Approvals` card with per-order Approve / Deny.
- **Live event feed (S9d)** — `GET /v1/events` is a Server-Sent Events stream (viewer role,
  same bearer auth as every route). Each frame carries a JSON array of audit-chain rows
  appended since the last one; an empty array plus a keep-alive comment when nothing changed.
  The dashboard consumes it with a `fetch`-based SSE reader (so the token stays in a header,
  which `EventSource` can't do) and shows activity from the stream, dropping the `/v1/activity`
  poll to a slow fill-count/fallback. SSE rather than a WebSocket — one-directional, native
  browser reconnect, no upgrade handshake to secure. `sherwood serve` stays a control plane
  and does not run the trading loop (see `docs/DECISIONS.md`).
- **Server serves the dashboard (S10.1)** — `[server] static_dir` (e.g. `frontend/dist`)
  makes `sherwood-server` serve the built dashboard at `/` with SPA fallback to
  `index.html`, so the whole thing runs from one loopback origin. Static responses carry a
  strict `Content-Security-Policy` (matching the build-time one), `X-Frame-Options: DENY`,
  `X-Content-Type-Options: nosniff`, and `Referrer-Policy: no-referrer`. `/v1/*` still takes
  precedence; config validation checks the directory has an `index.html`.
- **Dashboard (S10)** — `frontend/`: a React + Vite + TypeScript control panel for
  `sherwood-server`. Token login held in `sessionStorage` (this tab only, never on disk),
  dropped on any `401`. Status bar with the always-visible PAPER / **LIVE** badge (live is
  red and pulses) and kill-switch indicator; portfolio card (cash, realized P&L, positions);
  activity list with a chain-integrity badge from `GET /v1/audit/verify`; admin controls for
  the kill switch and the PAPER/LIVE toggle, each re-prompting for the admin token. No UI
  framework — one stylesheet in the shadcn idiom. Strict CSP injected at build time. New CI
  job runs `npm ci` · lint · typecheck · build. See
  [docs/FRONTEND-ARCH.md](docs/FRONTEND-ARCH.md).
- **Read-only state views on the server (S11a)** — `sherwood serve` opens the same
  `[general] state_path` database `sherwood run` writes, and exposes it: `GET /v1/portfolio`
  (cash, realized P&L, open positions), `GET /v1/activity?limit=N` (recent audit-chain
  events + fill count), `GET /v1/audit/verify` (recompute the hash chain — `{ ok, entries }`
  or `{ ok: false, broken_at }`). All viewer-role; `404` through the envelope when no
  `state_path` is configured or no snapshot exists yet. Gives the dashboard (S10) real data
  before the full approval gate.
- **Metrics, rate limit, CORS (S9c)** — `GET /v1/metrics` returns Prometheus text
  (hand-rolled counters — request totals, status-class breakdown, uptime, plus `kill_switch`
  and `mode_live` gauges; no `prometheus` crate). A global fixed-window rate limit
  (`[server] rate_limit_per_min`, default 120, `0` disables) returns `429` through the error
  envelope. `[server] cors_origins` allows the dashboard's browser origin; empty = same-origin
  only. The WebSocket event feed and generated OpenAPI are deferred to S9d — both want the
  run loop folded into the server first.
- **RBAC, PAPER/LIVE toggle, kill switch (S9b)** — `sherwood-server` gains three roles
  (`viewer` < `operator` < `admin`), assigned by which configured token authenticates
  (`token_ref` = admin; optional `operator_token_ref` / `viewer_token_ref`). `require_auth`
  middleware stamps the role; each route declares its minimum via a `Caller` extractor. New
  routes: `GET /v1/control` (viewer), `POST /v1/mode` and `POST /v1/kill` — both admin **and**
  the admin token again in the request body (`reauth`). LIVE mode is refused unless
  `[server] allow_live = true`. The kill switch flips `RiskConfig.kill_switch` under an
  `RwLock<Control>`, so an engaged switch immediately makes `POST /v1/hook/pretooluse` deny
  every order. `/v1/health` now reports `kill_switch`.
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
