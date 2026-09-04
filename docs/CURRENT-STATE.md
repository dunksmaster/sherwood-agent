---
status: accepted
last-updated: 2026-09-04
owner-step: S0
---

# Current state

An honest audit of the scaffold as it exists today. The gap table originated in an external
review (see [reviews/2026-09-03-kimi-master-plan.md](reviews/2026-09-03-kimi-master-plan.md))
and was verified against the source.

## What exists

| Crate | Real | Placeholder |
|---|---|---|
| `sherwood-core` | Domain types; `Portfolio` (serde, avg-cost, realized + **unrealized P&L**, `open_position_count`); `RiskGate` (hard stops + entry limits via `GateContext`); injected `Clock`; **`PriceFeed` trait + `Tick`**. 16 unit tests. | `proptest` on the gate arithmetic (S5.7); a `RiskGate` reset path after a breaker fires |
| `sherwood-store` | `Store` trait; `SqliteStore` (`sqlx`, compile-time-checked queries, embedded migrations): portfolio snapshots, fill history, **hash-chained tamper-evident audit log** with `verify_audit_chain`. `StoreSubscriber` persists straight from the event bus. 8 unit tests incl. kill-and-restart and tamper detection. | `config_state` / `cursors` / `pending_approvals` tables (added at S2 / S5 / S11); external anchoring of the chain head |
| `sherwood-secrets` | Encrypted file vault: Argon2id key + XChaCha20-Poly1305 over a JSON `name → value` map; passphrase from `$SHERWOOD_VAULT_PASSPHRASE`; `SecretString` zeroes on drop and prints `[redacted]`; `resolve_ref` turns `vault:NAME` config refs into values. `sherwood secrets set/get/list/rm`. 7 tests incl. wrong-passphrase and tamper detection. | OS keyring backend (behind a feature, when wanted); rotation tooling |
| `sherwood-events` | Internal bus (`tokio::sync::broadcast`, bounded 1000). `Event` (4 variants, each with a real emitter and consumer), versioned `Envelope`, `Subscriber` trait + `run_subscriber`, `TracingSubscriber`. A slow or failing subscriber is logged, never fatal. 4 unit tests. | Metrics / notification subscribers (S13); supervisor (S3.4–3.5) |
| `sherwood-execution` | `Executor` trait, deterministic `PaperExecutor` (spread + fee + slippage guard), `LiveExecutor` that always errors. **`hook`** — the fail-closed `PreToolUse` decision core for [ADR-0001](adr/0001-mcp-interaction-model.md) Option 3: `ToolAllowlist` (config-driven, no baked-in tool names) + `HookGate::evaluate` returning allow/deny; unknown tool, unparseable order args, or a `RiskGate` reject all deny; reads and cancels pass (cancels even under a hard stop). **`order_parse`** — strict agent-args → `core::Order`. 28 unit tests. | Retry / circuit breaker, order lifecycle beyond a synchronous `Fill` |
| `sherwood-server` | axum on loopback (non-loopback bind refused). One `{code,message,correlation_id}` error envelope; internal errors never leak detail. Bearer auth — `ApiToken` (constant-time via `subtle`, zeroised, generated into the vault on first run), **three RBAC roles** by which configured token matched, `Caller` extractor per route. `GET /v1/health` (open, reports mode + kill-switch), `GET /v1/control` (viewer), `POST /v1/hook/pretooluse` (operator) runs `HookGate` against a caller-supplied portfolio/market context — a *denied* call is a `200` body, only a malformed request is `4xx`, `POST /v1/mode` + `POST /v1/kill` (admin **and** body re-auth; LIVE gated by `[server] allow_live`). `RwLock<Control>` so an engaged kill switch denies orders immediately. `GET /v1/metrics` (Prometheus text, hand-rolled), a global fixed-window rate limit (`429` through the envelope), CORS for configured origins. **Read-only views** over the `[general] state_path` DB `sherwood run` writes: `GET /v1/portfolio`, `GET /v1/activity`, `GET /v1/audit/verify` (all viewer; `404` when no store). `GET /v1/events` — an SSE stream of new audit-chain rows (viewer). **Approval gate** ([ADR-0005](adr/0005-approval-gate.md)): `[server] approval_mode = "manual"` holds a risk-passing order via `POST /v1/hook/pretooluse` until the operator decides through `GET /v1/approvals` + `POST /v1/approvals/{id}`, or a timeout auto-denies; in-memory `pending → approved\|denied\|expired`, 5s sweeper. **Per-session budgets**: `[server]` order-count / notional / duration hard stops that latch a deny on every place-order until `POST /v1/session/reset` (admin); `GET /v1/session` usage. Optionally serves the built dashboard at `/` (`[server] static_dir`, SPA fallback, strict CSP + `X-Frame-Options` / `nosniff` / `Referrer-Policy`). `sherwood serve <config>` — a control plane; it does not run the trading loop. 45 unit tests. | `executed`/`settled` approval states (need order reconciliation, S7.4); the cron scheduler + price monitors (S12b, need the run loop); generated OpenAPI (S9e) |
| `frontend/` | React + Vite + TypeScript dashboard, no UI framework (one shadcn-idiom stylesheet). Session-only token login; status bar with the PAPER/**LIVE** badge + kill-switch indicator; portfolio, activity (fed by the `/v1/events` SSE stream), audit-integrity, and **approvals** (per-order Approve / Deny) views; admin kill-switch + mode toggle with re-auth. `usePoll` over `fetch`; strict build-time CSP; Vite dev proxy to `:8787`. CI job: `npm ci` · lint · typecheck · build. | Config editor (needs a config API); charts |
| `sherwood-decision` | `Decider` trait; `RuleDecider` (momentum entry, take-profit, stop-loss, liquidity floor); **`AiDecider`** — either a caller-supplied async closure or, via `from_provider`, an `AiProvider` with the crate-owned prompt, `<market_data>` injection guard, strict-JSON parser (`deny_unknown_fields`, fence strip, one retry, degrade-to-Hold), per-run call budget and optional symbol universe. `OpenAiCompatProvider` (`reqwest`/rustls, request timeout) behind the `openai` feature. 20 unit tests. | No cross-run quota manager (S4.7, deferred — per-run budget covers v0.1); Claude/Groq need no separate provider (both OpenAI-compatible) |
| `sherwood-copytrade` | `TradeFeed` trait, `ObservedTrade`, `CopyTrader` with three sizing modes and sell clamping. 5 unit tests. | No live `TradeFeed` impl. **Not wired into the runner.** Deferred to v0.2 |
| `sherwood-sniper` | `NewPoolEvent`, `RugScreen` with 7 safety checks, entry-order builder. 4 unit tests. | No pool event source. **Not wired into the runner.** Deferred to v0.2 |
| `sherwood-cli` | `demo` / `run` / `backtest` / `serve` / `check`; validated TOML config; paper-only guard; clean Ctrl-C. **Multi-asset run loop** driven by a `PriceFeed` — the built-in two-symbol demo feed, or a **CSV replay** (`feed_path`). `build_decider` selects `RuleDecider` or the AI decider from `[general] decider`, resolving `ai.api_key` against the vault. `backtest` replays the feed through the same loop with a `Recording` and prints total return / drawdown / win rate / profit factor / expectancy ([BACKTEST.md](BACKTEST.md)). `serve` starts `sherwood-server` from `[server]` + `[hook]`. Publishes events onto the bus; a tracing subscriber logs them and (with `state_path`) a store subscriber persists them and the loop resumes from the last snapshot. 29 tests. | Live feed (v0.2); copy-trade and sniper config not wired; naive `change_24h` (previous tick, not a real window) |

161 Rust tests pass. `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D
warnings`, and `cargo deny check` (licences + RustSec advisories + bans + sources) are all
clean. CI runs those plus an MSRV 1.80 build, a CycloneDX SBOM, `gitleaks`, a coverage
report, a doc-link check, and the frontend job (`npm ci` · ESLint · `tsc` · `vite build`).
`sqlx` queries are compile-time-checked against the committed `.sqlx/` offline cache, so CI
needs no database.

## What is good — keep

- **`RiskGate` as a single choke point.** Every order passes one gate. Correct safety shape.
- **Paper-only default.** `LiveExecutor` is an intentional stub; `config.rs` hard-rejects any
  `mode` that is not `"paper"`. No accidental live trading is possible today.
- **`rust_decimal` everywhere for money.** No float drift anywhere in the tree.
- **Trait-based seams.** `Executor`, `Decider`, `TradeFeed` are all swappable.
- **Clean crate separation.** One job per crate, no circular dependencies, `core` has no I/O.

## Known defects and gaps

Severity is relative to shipping v0.1.

| # | Gap | Severity | Detail |
|---|---|---|---|
| 1 | ~~No persistence layer~~ | ~~Blocker~~ | **Closed (S1):** `sherwood-store` on SQLite. `Portfolio` is `serde`-serialisable; `run` with `state_path` snapshots it, records fills, and resumes on restart. A kill-and-restart test proves the round-trip. Multi-node / Postgres is a later concern. |
| 2 | ~~No real data feed~~ | ~~Blocker~~ | **Closed (S5):** `PriceFeed` trait in `core`; `CsvFeed` replays `timestamp,symbol,price` rows, `SliceFeed` replays in memory. `run` uses `feed_path` or the built-in demo feed. A live websocket/Geyser feed is v0.2. |
| 3 | ~~Hardcoded single asset~~ | ~~Blocker~~ | **Closed (S5):** the loop is multi-asset — the feed defines the universe (one symbol per tick), equity and unrealized P&L mark against the latest price per held symbol. The demo feed trades two. |
| 4 | Config partly unused | **High** | `run()` consumes `general` (incl. `decider` + the `[ai]` section), the risk section, `state_path` and `feed_path`. Copy-trade leaders and sniper settings are still logged and discarded — both are v0.2. |
| 5 | Strategies not wired | **Blocker** | `CopyTrader` and `RugScreen` are library-only. Neither is reachable from the binary. |
| 6 | ~~No event bus~~ | ~~Blocker~~ | **Closed (S3):** `sherwood-events` — bounded `broadcast`, versioned envelopes, `Subscriber` trait. The run loop publishes; `TracingSubscriber` and `StoreSubscriber` consume. Adding metrics or notifications is a new subscriber, no producer change. Supervisor (config-driven startup) deferred to S4 — nothing to supervise yet. |
| 7 | ~~No audit log~~ | ~~High~~ | **Closed (S1):** hash-chained `audit_log` in `sherwood-store`, fed via the bus (S3). `verify_audit_chain` walks from genesis and pinpoints the first altered row. A tamper test confirms detection. External anchoring of the head is a later hardening task. |
| 8 | ~~`RiskGate` ignores unrealized P&L~~ | ~~High~~ | **Closed (S5):** the gate takes a `GateContext` with mark-to-market unrealized P&L. Below `-max_unrealized_loss`, new **buys** are refused; sells (de-risking) still pass. |
| 9 | ~~No graceful shutdown~~ | ~~High~~ | **Fixed (Pre-S0):** a Ctrl-C handler sets a flag; `run_loop` stops cleanly at the next tick and still prints the ledger. Full state *flush* waits on persistence (item 1). |
| 10 | ~~Weak config validation~~ | ~~High~~ | **Fixed (Pre-S0):** `AppConfig::validate` runs on every load — range checks on every numeric field, allow/deny overlap check, actionable errors. Six tests. |
| 11 | No retry or circuit breaker | High | `PaperExecutor` can fail; there is no policy for what happens next. |
| 12 | ~~No cooldown or max-open-positions~~ | ~~Medium~~ | **Closed (S5):** `max_open_positions` refuses a buy that would open a new symbol past the cap; `order_cooldown_secs` refuses a same-symbol buy inside the window (`now` from an injected `Clock`). Sells are never gated by either. |
| 13 | Order lifecycle is synchronous | Medium | `Executor::execute` returns a `Fill` immediately. A real venue returns an order id whose status must be polled. |
| 14 | ~~No shutdown flush of the ledger~~ | ~~Medium~~ | **Closed (S1):** when `state_path` is set, `run` snapshots the portfolio and writes a `run_end` audit event on exit, including on a clean interrupt. |

Items 4 and 5 now concern only copy-trade and sniper, which are v0.2. Items 11 and 13 are
addressed in S7. Everything else in this table has been closed across S0–S6.

Also fixed in Pre-S0: `PaperExecutor` recovers from a poisoned mutex instead of unwrapping,
and `run_loop` guards an empty price series instead of indexing.

## Environment notes

- Rust toolchain: `stable-x86_64-pc-windows-msvc`. MSVC C++ Build Tools required.
- On Windows, build from **PowerShell** — Git Bash's coreutils `link` shadows MSVC `link.exe`
  and produces a confusing "extra operand" linker error.
- `rustup` and VS 2022 Build Tools were installed on the development machine on 2026-09-03.
