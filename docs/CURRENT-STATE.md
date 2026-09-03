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
| `sherwood-core` | Domain types (`Asset`, `Order`, `Fill`, `Decision`, `Signal`), `Portfolio` ledger with avg-cost and realized P&L, `RiskGate` with 8 rejection reasons. 6 unit tests. | Unrealized P&L; persistence; multi-asset equity helper is a caller-supplied closure |
| `sherwood-execution` | `Executor` trait, deterministic `PaperExecutor` (spread + fee + slippage guard), `LiveExecutor` that always errors. 3 unit tests. | Retry, circuit breaker, order lifecycle beyond a synchronous `Fill` |
| `sherwood-decision` | `Decider` trait, `RuleDecider` (momentum entry, take-profit, stop-loss, liquidity floor), `AiDecider` wrapping a caller-supplied async closure. 5 unit tests. | No provider client, no prompt, no output schema — **by design**, the closure is the seam |
| `sherwood-copytrade` | `TradeFeed` trait, `ObservedTrade`, `CopyTrader` with three sizing modes and sell clamping. 5 unit tests. | No live `TradeFeed` impl. **Not wired into the runner.** Deferred to v0.2 |
| `sherwood-sniper` | `NewPoolEvent`, `RugScreen` with 7 safety checks, entry-order builder. 4 unit tests. | No pool event source. **Not wired into the runner.** Deferred to v0.2 |
| `sherwood-cli` | `demo` / `run` / `check` commands; TOML config parsing **with full validation** (range + overlap checks, 6 tests); paper-only mode guard; Ctrl-C stops a run cleanly | Hardcoded single asset; synthetic price series; copy-trade and sniper config fields still not wired to behaviour |

32 tests pass. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
`cargo deny check` (licences + RustSec advisories + bans + sources) are all clean. CI runs
those plus an MSRV 1.80 build, a CycloneDX SBOM, `gitleaks`, a coverage report, and a
doc-link check.

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
| 1 | No persistence layer | **Blocker** | `Portfolio` is in-memory only. A crash loses all state. No `sherwood-store`. |
| 2 | No real data feed | **Blocker** | `runner.rs::synthetic_series()` is a hardcoded 8-point price path. |
| 3 | Hardcoded single asset | **Blocker** | `runner.rs` uses `Asset::symbol("ROAR")` in both `demo()` and `run()`. Multi-asset config is ignored. |
| 4 | Config is parsed but largely unused | **Blocker** | `run()` reads `starting_cash` and the risk section; copy-trade leaders and sniper settings are logged and discarded. |
| 5 | Strategies not wired | **Blocker** | `CopyTrader` and `RugScreen` are library-only. Neither is reachable from the binary. |
| 6 | No event bus | **Blocker** | Components call each other directly. No decoupling, no audit subscriber, no metrics subscriber. |
| 7 | No audit log | High | Nothing is persisted. The plan calls for a hash-chained log; none exists. |
| 8 | `RiskGate` ignores unrealized P&L | High | Only `realized_pnl` feeds the daily-loss breaker. An open position can bleed arbitrarily while the gate still admits new entries. |
| 9 | ~~No graceful shutdown~~ | ~~High~~ | **Fixed (Pre-S0):** a Ctrl-C handler sets a flag; `run_loop` stops cleanly at the next tick and still prints the ledger. Full state *flush* waits on persistence (item 1). |
| 10 | ~~Weak config validation~~ | ~~High~~ | **Fixed (Pre-S0):** `AppConfig::validate` runs on every load — range checks on every numeric field, allow/deny overlap check, actionable errors. Six tests. |
| 11 | No retry or circuit breaker | High | `PaperExecutor` can fail; there is no policy for what happens next. |
| 12 | No cooldown or max-open-positions | Medium | Nothing prevents overtrading a single symbol or opening unbounded concurrent positions. |
| 13 | Order lifecycle is synchronous | Medium | `Executor::execute` returns a `Fill` immediately. A real venue returns an order id whose status must be polled. |
| 14 | No shutdown flush of the ledger | Medium | The interrupt now stops cleanly (item 9), but there is still nowhere to flush to (item 1). |

Items 1–6 are addressed by S1–S5 of the [roadmap](ROADMAP.md). Items 8 and 12 are addressed
by the `RiskGate` extension in S5. Items 11 and 13 are addressed in S5 and S7. Items 9 and 10
were closed in the Pre-S0 hygiene pass.

Also fixed in Pre-S0: `PaperExecutor` recovers from a poisoned mutex instead of unwrapping,
and `run_loop` guards an empty price series instead of indexing.

## Environment notes

- Rust toolchain: `stable-x86_64-pc-windows-msvc`. MSVC C++ Build Tools required.
- On Windows, build from **PowerShell** — Git Bash's coreutils `link` shadows MSVC `link.exe`
  and produces a confusing "extra operand" linker error.
- `rustup` and VS 2022 Build Tools were installed on the development machine on 2026-09-03.
