---
status: accepted
last-updated: 2026-09-03
owner-step: S0
---

# Current state

An honest audit of the scaffold as it exists today. The gap table originated in an external
review (see [reviews/2026-09-03-kimi-master-plan.md](reviews/2026-09-03-kimi-master-plan.md))
and was verified against the source.

## What exists

| Crate | Real | Placeholder |
|---|---|---|
| `sherwood-core` | Domain types; `Portfolio` (serde, avg-cost, realized + **unrealized P&L**, `open_position_count`); `RiskGate` — hard stops (kill switch, daily loss) + entry limits (notional, position fraction, **unrealized-loss breaker, max open positions, per-symbol cooldown**) via a `GateContext`; injected `Clock` (`SystemClock` / `FixedClock`). 14 unit tests. | `proptest` on the gate arithmetic (S5.7); a `RiskGate` reset path after a breaker fires |
| `sherwood-store` | `Store` trait; `SqliteStore` (`sqlx`, compile-time-checked queries, embedded migrations): portfolio snapshots, fill history, **hash-chained tamper-evident audit log** with `verify_audit_chain`. `StoreSubscriber` persists straight from the event bus. 8 unit tests incl. kill-and-restart and tamper detection. | `config_state` / `cursors` / `pending_approvals` tables (added at S2 / S5 / S11); external anchoring of the chain head |
| `sherwood-events` | Internal bus (`tokio::sync::broadcast`, bounded 1000). `Event` (4 variants, each with a real emitter and consumer), versioned `Envelope`, `Subscriber` trait + `run_subscriber`, `TracingSubscriber`. A slow or failing subscriber is logged, never fatal. 4 unit tests. | Metrics / notification subscribers (S13); supervisor (S3.4–3.5) |
| `sherwood-execution` | `Executor` trait, deterministic `PaperExecutor` (spread + fee + slippage guard), `LiveExecutor` that always errors. 3 unit tests. | Retry, circuit breaker, order lifecycle beyond a synchronous `Fill` |
| `sherwood-decision` | `Decider` trait, `RuleDecider` (momentum entry, take-profit, stop-loss, liquidity floor), `AiDecider` wrapping a caller-supplied async closure. 5 unit tests. | No provider client, no prompt, no output schema — **by design**, the closure is the seam |
| `sherwood-copytrade` | `TradeFeed` trait, `ObservedTrade`, `CopyTrader` with three sizing modes and sell clamping. 5 unit tests. | No live `TradeFeed` impl. **Not wired into the runner.** Deferred to v0.2 |
| `sherwood-sniper` | `NewPoolEvent`, `RugScreen` with 7 safety checks, entry-order builder. 4 unit tests. | No pool event source. **Not wired into the runner.** Deferred to v0.2 |
| `sherwood-cli` | `demo` / `run` / `check`; validated TOML config; paper-only guard; clean Ctrl-C. The run loop **publishes events onto the bus**; a tracing subscriber always logs them, and (when `state_path` is set) a store subscriber persists them and the loop resumes from the last snapshot. 9 tests. | Hardcoded single asset; synthetic price series; copy-trade and sniper config fields still not wired to behaviour |

53 tests pass. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
`cargo deny check` (licences + RustSec advisories + bans + sources) are all clean. CI runs
those plus an MSRV 1.80 build, a CycloneDX SBOM, `gitleaks`, a coverage report, and a
doc-link check. `sqlx` queries are compile-time-checked against the committed `.sqlx/`
offline cache, so CI needs no database.

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
| 2 | No real data feed | **Blocker** | `runner.rs::synthetic_series()` is a hardcoded 8-point price path. |
| 3 | Hardcoded single asset | **Blocker** | `runner.rs` uses `Asset::symbol("ROAR")` in both `demo()` and `run()`. Multi-asset config is ignored. |
| 4 | Config is parsed but largely unused | **Blocker** | `run()` reads `starting_cash` and the risk section; copy-trade leaders and sniper settings are logged and discarded. |
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

Items 2–5 are addressed by S2–S5 of the [roadmap](ROADMAP.md). Items 8 and 12 were closed by
the `RiskGate` extension in S5. Items 11 and 13 are addressed in S5 and S7. Items 9 and 10
were closed in the Pre-S0 hygiene pass.

Also fixed in Pre-S0: `PaperExecutor` recovers from a poisoned mutex instead of unwrapping,
and `run_loop` guards an empty price series instead of indexing.

## Environment notes

- Rust toolchain: `stable-x86_64-pc-windows-msvc`. MSVC C++ Build Tools required.
- On Windows, build from **PowerShell** — Git Bash's coreutils `link` shadows MSVC `link.exe`
  and produces a confusing "extra operand" linker error.
- `rustup` and VS 2022 Build Tools were installed on the development machine on 2026-09-03.
