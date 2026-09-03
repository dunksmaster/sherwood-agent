---
status: accepted
last-updated: 2026-09-03
owner-step: S0
---

# Engineering standards

These are gates, not aspirations. CI enforces what can be enforced; the rest is review.

## Toolchain

- **MSRV: 1.80.** Pinned in `rust-toolchain.toml` and asserted in CI. Raising it is a PR of
  its own with a stated reason.
- **Edition 2021.** Host toolchain on Windows is `stable-x86_64-pc-windows-msvc`.
- Build from PowerShell on Windows — Git Bash's coreutils `link` shadows MSVC `link.exe`.

## Workspace lints

Declared once in the root `Cargo.toml` and inherited by every crate via
`[lints] workspace = true`.

**Enforced now** — CI runs `clippy --all-targets --all-features -- -D warnings`, so every
lint below is effectively a build failure:

```toml
[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
unwrap_used   = "deny"
expect_used   = "deny"
panic         = "deny"
todo          = "deny"
unimplemented = "deny"
dbg_macro     = "deny"
```

Each crate root carries
`#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]` so test
code may still `unwrap` freely.

**Phased in per crate** as the codebase matures — added to a crate's own `[lints]` once it
passes, rather than workspace-wide where a single laggard crate would block CI:

- `missing_docs = "warn"` — as public items get doc comments
- `unreachable_pub = "warn"` — after the `cli` binary's internal items move to `pub(crate)`
- `clippy::pedantic` — crate by crate, fixing the surfaced style lints as you go
- `clippy::float_arithmetic = "deny"` — money is already `Decimal` everywhere (a review
  rule); the lint is added once confirmed noise-free per crate

`unsafe_code` may only be relaxed per-crate, with a `SAFETY:` comment on every block and an
ADR explaining why. No such crate exists today.

`unwrap` / `expect` / `panic` are permitted in `#[cfg(test)]` code and in `build.rs` only.
Non-test code that must lock a `Mutex` recovers from poisoning
(`.lock().unwrap_or_else(|e| e.into_inner())`) rather than unwrapping.

## Errors

- `thiserror` for library crates — typed, exhaustive, matchable.
- `anyhow` **only** at the binary edge (`sherwood-cli`, `sherwood-server` handlers).
- Every fallible function returns a typed error. No stringly-typed failures.
- Every error is classified per the taxonomy in
  [ARCHITECTURE.md](ARCHITECTURE.md#error-taxonomy): `Transient`, `Fatal`, `Rejected`,
  `Invariant`. The classification decides the response; the call site does not improvise.

## Money

- **`rust_decimal` only.** Enforced by review now; by the `clippy::float_arithmetic` lint
  as it is phased into each crate.
- Prices, quantities, notionals, fees, and P&L are `Decimal` end to end, including over the
  HTTP API and in the database (stored as `TEXT`, not `REAL`).

## Determinism

Backtest, paper, and live share the same strategy and gate code. That only holds if nothing
in that path reads ambient state.

- Time comes from an injected `Clock` trait. `Utc::now()` never appears in `core`,
  `decision`, or gate code.
- Randomness comes from an injected seeded RNG.
- All I/O sits behind a trait with a test double.

A test asserting against `Utc::now()` is a defect.

## Testing

- Unit tests beside the code; integration tests in `tests/`.
- **`proptest` on the risk gate and all sizing arithmetic.** These are the places where a
  subtle error costs money.
- Every bug fix ships with a regression test that fails before the fix.
- Deterministic only — seeded RNG, injected clock, no network, no wall-clock sleeps.
- Coverage is tracked and visible, not hard-gated: `core` ≥ 80%, `execution` ≥ 70%,
  `decision` ≥ 60%. A PR that lowers coverage explains why.

## Feature flags

Optional dependencies sit behind features so a minimal build stays small and the attack
surface stays proportional to what is enabled:

`nvidia` · `claude` · `groq` · `prometheus` · `sqlcipher` · `notifications`

Default features are the minimum needed to run paper mode.

## Observability

- `tracing` with JSON output in production, human-readable in development.
- Every log line carries a **correlation id** — the decision id, then the order id it becomes.
  Propagated through the event bus.
- Every external call is wrapped in a span and records latency, outcome, and retry count.
- One span covers decision → gate → approval → execution so a single trade is one trace.

## API

- `utoipa`-generated OpenAPI. [`API.md`](API.md) is generated, never hand-written.
- Routes are versioned: `/v1/…`. Breaking changes bump the prefix.
- One error envelope across every endpoint: `{ code, message, correlation_id }`.
- Semantic versioning on the crate and the API together.

## Dependencies

- **Permissive licences only:** MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unlicense.
  **Banned:** GPL, AGPL, LGPL, ELv2, BSL, CC-BY-SA, anything non-commercial.
  Enforced by `.deny.toml`. See [LICENSING.md](LICENSING.md).
- Lockfiles committed. Versions pinned. Renovate opens update PRs.
- New dependencies are justified in the PR description: what it does, why not std, licence.

## Process

- **No direct push to `main`.** PR, green CI, review checklist.
- **Conventional commits** — `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`.
- `CHANGELOG.md` maintained from those commits.
- **An ADR for every irreversible decision.** MADR format, immutable once accepted.
- Signed tags for releases.

## CI gates

Every one of these must pass before merge:

| # | Gate |
|---|---|
| 1 | `cargo fmt --all --check` |
| 2 | `cargo clippy --all-targets --all-features -- -D warnings` |
| 3 | `cargo test --workspace --all-features` |
| 4 | `cargo deny check` — licences, advisories, bans, sources |
| 5 | `cargo audit` |
| 6 | `cargo cyclonedx` — SBOM artifact |
| 7 | `gitleaks detect` — secret scan |
| 8 | MSRV build |
| 9 | Doc-link check — every relative link in `docs/` resolves |
| 10 | Frontend: `tsc --noEmit`, lint, build (from S10) |

Pre-commit hooks run 1, 2, and 7 locally so CI is not the first place a failure appears.

## Review checklist

Applied to every PR:

- [ ] **No scaffold, no padding.** Does this PR add behaviour a user or a test could observe?
      If it only adds infrastructure "for later" — a trait with no working impl, a `pub` item
      with no caller, a parsed-but-unused config field — it is too early. See
      [DEFINITION-OF-DONE.md](DEFINITION-OF-DONE.md).
- [ ] Does any new order path bypass `RiskGate`? (If yes, reject.)
- [ ] Are new errors classified into the taxonomy?
- [ ] Is money `Decimal` throughout?
- [ ] Any `Utc::now()` or unseeded randomness in strategy or gate code?
- [ ] Are secrets absent from logs, errors, and API responses?
- [ ] Does a new dependency carry a permissive licence?
- [ ] Is there a regression test for the bug being fixed?
- [ ] Does an irreversible decision need an ADR?
