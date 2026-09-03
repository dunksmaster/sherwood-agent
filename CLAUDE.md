# CLAUDE.md

Context for AI coding assistants working in this repository.

## What this is

An automated trading system for the **Robinhood Agentic Trading MCP** (v0.1). Solana modules
are a later v0.2 milestone in the same repo. Read `docs/README.md` for the full index;
`docs/ROADMAP.md` for what is being built and in what order.

## Non-negotiable invariants

1. **No order path may bypass `RiskGate::check`.** Reject any change that lets an order reach
   an executor without it. This is the reason the project exists.
2. **Every control fails closed.** Unreachable, errored, or ambiguous means "do not trade".
3. **`PAPER` is the default and the only mode the CLI permits.** Do not loosen the mode guard
   in `crates/cli/src/config.rs`.
4. **No credentials in the codebase, the database, logs, or API responses.**
5. **Money is `rust_decimal::Decimal`.** Never `f32` / `f64` for prices, quantities, fees, P&L.

## Standards (enforced in CI)

- `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo test --workspace`, `cargo deny check`, `gitleaks`.
- Workspace lints forbid `unsafe_code` and deny `unwrap_used` / `expect_used` / `panic` /
  `todo` / `unimplemented` / `dbg_macro` in non-test code. Test code may `unwrap` (allowed via
  `#![cfg_attr(test, allow(...))]` in each crate root).
- `thiserror` for library errors, `anyhow` only at the binary edge.
- Inject `Clock` and RNG — no `Utc::now()` in strategy or gate code. Backtest == live.
- MSRV 1.80.

Full detail: `docs/ENGINEERING-STANDARDS.md`.

## Definition of done

A step is not done with stubs. Vertical slices, no `todo!` on `main`, every `pub` item has a
caller or a behavioural test, docs never lead the code. See `docs/DEFINITION-OF-DONE.md`.

## Decisions

Irreversible choices are ADRs in `docs/adr/`, MADR format, **immutable once accepted**. To
change one, write a superseding ADR. Accepted so far: agent-harness MCP model (0001),
advisory AI mode (0002), SQLite via `sqlx` (0003).

## Build notes

- On Windows, build from **PowerShell**, not Git Bash — Git Bash's coreutils `link` shadows
  the MSVC linker.
- `Cargo.lock` is committed; keep it in sync.
- Deferred crates `sniper` and `copytrade` are real tested logic, intentionally not wired.
  Do not delete them or treat them as stubs; see each crate's `README.md`.

## Process

- No direct push to `main`. Branch, PR, green CI. Conventional commits. Update `CHANGELOG.md`.
- Update `docs/CURRENT-STATE.md` in the same PR as any change to what is real vs. planned.
