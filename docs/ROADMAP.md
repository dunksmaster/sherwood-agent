---
status: accepted
last-updated: 2026-09-03
owner-step: S0
---

# Roadmap

## Direction

**v0.1 targets the Robinhood Agentic Trading MCP only.** No Solana, no wallets, no private
keys — Robinhood authenticates over OAuth and custodies the assets.

**v0.2 adds Solana** as additive modules in this same repository, behind the same `Executor`
and feed traits. It is not a rewrite.

The two are kept in one repo because the core — types, `RiskGate`, `Portfolio`, decision
layer, server, dashboard, approvals, scheduler, audit — is shared. Only the venue adapters
differ, and those are already isolated behind traits.

## MVP — v0.1

An AI decision layer proposes trades for your Robinhood Agentic account, on a schedule or in
response to a monitored event. Every proposal clears the `RiskGate` and the spend caps. In
**manual mode** you approve each order; in **auto mode** it executes within configured
limits. A local dashboard shows portfolio, activity, pending approvals, and a kill switch.
Every decision, order, and fill is written to a tamper-evident audit log.

Paper is the default. Live requires the MCP connected, the admin role, and an explicit
toggle.

### Explicitly not in v0.1

Solana · sniper · copy-trading · multi-user · cloud or multi-node deployment · dynamic
strategy plugin loading · HSM or hardware custody · compliance tooling.

## Phases

```mermaid
flowchart LR
    P0[Pre-S0<br/>Repo bootstrap] --> S0[S0<br/>Governance]
    S0 --> F[S1-S3<br/>Foundation]
    F --> D[S4-S5<br/>Decision + paper]
    D --> R[S6-S8<br/>Robinhood]
    R --> O[S9-S13<br/>Ops shell]
    O --> H[S14-S16<br/>Hardening + release]
    H --> V2[v0.2<br/>Solana]
```

Each phase gates the next. **No code is written until S0 is complete and reviewed.**

## Pre-S0 — repo bootstrap

Also the hygiene half of the [grade target](GRADE-TARGET.md) — these lift the process
dimensions of the rubric without any architecture work. The `H` labels are cross-referenced
from `GRADE-TARGET.md`.

| Step | Task | Grade item |
|---|---|---|
| P0.1 | ~~Decide repository visibility~~ — **public** (2026-09-04), open-source project | — |
| P0.2 | ~~Branch protection on `main`~~ — **done:** PR required, all CI checks required (strict), linear history, no force-push, no deletion, `enforce_admins = false` so the owner can hotfix in an emergency | H4 |
| P0.3 | Issue templates — bug, feature, security | — |
| P0.4 | PR template with the review checklist | H4 |
| P0.5 | `CODEOWNERS` | H4 |
| P0.6 | `.deny.toml` — allow MIT/Apache-2.0/BSD/ISC/Zlib, ban GPL/AGPL/LGPL/ELv2, check advisories | H3 |
| P0.7 | Renovate or Dependabot configuration | H2 |
| P0.8 | Pre-commit hooks: `gitleaks`, `cargo fmt`, `cargo clippy` | H3 |
| P0.9 | Remove `Cargo.lock` from `.gitignore` and commit it | H1 |
| P0.10 | `#![forbid(unsafe_code)]` + `[workspace.lints]` in the root `Cargo.toml` | H5 |
| P0.11 | `CHANGELOG.md` and tag `v0.0.1` | H6 |
| P0.12 | `cargo-llvm-cov` coverage reported in CI | H7 |
| P0.13 | `CLAUDE.md` for the repo | H8 |
| P0.14 | Config validation (defect 10) and graceful shutdown (defect 9) — small, real code | H9 |

## S0 — governance

| Step | Task | Output |
|---|---|---|
| S0.1 | Engineering standards, including the workspace lint manifest and pinned MSRV | `ENGINEERING-STANDARDS.md` |
| S0.2 | Security architecture and policy | `SECURITY.md` |
| S0.3 | Threat model — STRIDE plus MCP- and AI-specific threats | `THREAT-MODEL.md` |
| S0.4 | AI safety — prompt injection, output schema, budgets, provenance | `AI-SAFETY.md` |
| S0.5 | Contribution process and CLA | `CONTRIBUTING.md` |
| S0.6 | Licence strategy and provenance trail | `LICENSING.md`, `PRIOR-ART.md` |
| S0.7 | ADRs 0001–0003 seeded | `adr/` |
| S0.8 | Robinhood integration facts gathered | `ROBINHOOD-API.md` |
| S0.9 | CI workflow: fmt · clippy · test · deny · audit · SBOM · gitleaks · doc-link | `.github/workflows/ci.yml` |
| S0.10 | README rewritten for the v0.1 scope | `README.md` |

**Exit criteria:** ADR-0001 accepted. All Tier-1 docs written. CI gates green on a no-op PR.

## S1–S3 — foundation

| Step | Task | Crate | Status |
|---|---|---|---|
| S1.1 | Schema — `portfolio_snapshots`, `fills`, `audit_log`. (`cursors` / `config_state` / `pending_approvals` deferred to S5 / S2 / S11, since an unused table is scaffold.) | `store` | done |
| S1.2 | `Store` trait — no `UPDATE`/`DELETE` path for the audit table | `store` | done |
| S1.3 | `SqliteStore` via `sqlx`, compile-time-checked queries, committed `.sqlx/` offline cache (`SQLX_OFFLINE` in CI) | `store` | done |
| S1.4 | Hash-chained `audit_log` + `verify_audit_chain`; tamper test | `store` | done |
| S1.5 | External anchoring of the chain head | `store` | deferred (hardening) |
| S1.6 | `Portfolio` is `serde`-serialisable; JSON round-trip test; wired into `run` (resume from snapshot, persist fills + audit + exit snapshot) | `core`, `store`, `cli` | done |
| S2.1 | `AppConfig` with real validation — range checks, overlap checks, actionable errors | `cli` | done (Pre-S0) |
| S2.2 | Config reload via `notify`, broadcasting a change event | `config` | deferred to S4 — no long-lived config consumer yet |
| S2.3 | Config schema versioning with migration stubs | `config` | deferred to S4 |
| S2.4 | Secret references resolved at load time, never stored inline | `config` | deferred to S6 — needs the secrets vault |
| S3.1 | Event bus — `broadcast`, bounded 1000, backpressure policy, `Bus` handle | `events` | done |
| S3.2 | `Event` (real emitters + consumers only), versioned `Envelope` ([ADR-0004](adr/0004-event-schema-versioning.md)) | `events` | done |
| S3.3 | `Subscriber` trait + `run_subscriber`; `TracingSubscriber`; `StoreSubscriber`; run loop publishes instead of calling the store | `events`, `store`, `cli` | done |
| S3.4 | `Supervisor` trait — `start`, `stop`, `health_check` | `supervisor` | deferred to S4 — nothing multi-component to supervise yet |
| S3.5 | Config-driven component startup | `supervisor` | deferred to S4 |

## S4–S5 — decision layer and the paper loop

| Step | Task | Crate | Status |
|---|---|---|---|
| S4.1 | `Decider` selection — `general.decider = "rule" \| "ai"`, built in `cli::runner::build_decider` | `decision`, `cli` | done — a name→factory registry is deferred until a third decider exists |
| S4.2 | `AiProvider` trait + `AiError`; `OpenAiCompatProvider` (NVIDIA NIM / Groq / local) behind the `openai` feature, `reqwest` rustls, whole-request timeout | `decision` | done |
| S4.3 | `AiDecider::from_provider` — call budget, one retry, provider error → Hold | `decision` | done |
| S4.4 | Claude / Groq as distinct providers | `decision` | not needed — both are OpenAI-compatible and covered by `OpenAiCompatProvider` + `base_url`/`model` |
| S4.5 | Strict JSON output (`deny_unknown_fields`), fence strip, semantic validation, fallback-to-Hold chain | `decision` | done — see [AI-SAFETY.md](AI-SAFETY.md) |
| S4.6 | Prompt template — `<market_data>` delimited untrusted block, injection guard on the symbol field | `decision` | done |
| S4.7 | Shared AI quota manager (cross-run) | `supervisor` | deferred — per-run `max_calls_per_run` covers v0.1 |
| S5.1 | `PriceFeed` trait (`core`) + `CsvFeed` / `SliceFeed` (`cli`) | `core`, `cli` | done |
| S5.2 | Multi-asset run loop — the feed defines the universe; equity/unrealized mark per held symbol | `cli` | done |
| S5.3 | End-to-end loop over the event bus: feed → decider → gate → executor → store | `cli` | done (S1 + S3) |
| S5.4 | `RiskGate` extension: unrealized-loss breaker, max open positions, per-symbol cooldown, via `GateContext` | `core` | done |
| S5.5 | Kill switch wired — bus event plus gate check | `core`, `cli` | partial — gate check done; a runtime toggle needs the server (S9) |
| S5.6 | Graceful shutdown — SIGINT, stop cleanly, snapshot on exit | `cli` | done (Pre-S0 + S1) |
| S5.7 | Deterministic harness — injected `Clock` (done), seeded RNG (pending), `proptest` on gate arithmetic (pending) | `core`, `cli` | partial |

**Exit criteria — met:** `sherwood run` with a `feed_path` CSV replays a two-symbol paper
run, persists the portfolio + fills + a verifying audit chain, and resumes from the last
snapshot on the next run. With `decider = "ai"` the same loop is driven by an
OpenAI-compatible model, API key from the vault, every proposal still passing `RiskGate`.
Still open within S4–S5: a seeded RNG and `proptest` on the gate arithmetic.

## S6 — secrets vault

| Step | Task | Crate | Status |
|---|---|---|---|
| S6.1 | `SecretsVault` trait + `SecretString` (zeroes on drop); `resolve_ref` for `vault:NAME` config refs | `secrets` | done |
| S6.2 | `FileVault` — Argon2id + XChaCha20-Poly1305 encrypted file; passphrase from env. `sherwood secrets` CLI | `secrets`, `cli` | done |
| S6.3 | OS keyring backend | `secrets` | deferred — behind a feature, when there is demand; `FileVault` covers v0.1 |
| S6.4 | Local API token generated on first run | `secrets` | deferred to S9 — needs the server |

## S7–S8 — Robinhood integration

Per [ADR-0001](adr/0001-mcp-interaction-model.md) Option 3 (agent harness + fail-closed
`PreToolUse` hook), so "MCP client" below means the **hook decision core**, not an OAuth
client — the agent CLI owns the MCP connection.

| Step | Task | Crate | Status |
|---|---|---|---|
| S7.1 | Hook decision core — `ToolCall` in, `HookOutcome` (allow / deny) out; `evaluate_payload` for a raw body | `execution` | done — `execution::hook` |
| S7.2 | **Tool allowlist** — only classified tool names may be invoked, everything else denied; order tools are parsed + risk-checked, reads and cancels pass, cancels pass even under a hard stop | `execution` | done — `ToolAllowlist` / `HookGate` |
| S7.2a | Order-argument parser — agent tool args → `core::Order`, strict, unparseable ⇒ deny | `execution` | done — `execution::order_parse` |
| S7.3 | `RobinhoodExecutor` — place, cancel, status (Option 1 second mode; not on the v0.1 path) | `execution` | deferred — Option 3 has the agent place orders |
| S7.4 | Order-status reconciliation against the agentic order ledger | `execution` | pending — needs the live MCP |
| S7.5 | Portfolio, positions, and quote reads | `execution` | pending — needs the live MCP |
| S7.6 | Error taxonomy — retryable vs fatal, mapped to `ExecError` | `execution` | partial — deny reasons are structured; retry/fatal split is S8 |
| S7.7 | Rate-limit handling | `execution` | pending |
| S8.1 | Session state machine — disconnected → connecting → connected → active → stale | `execution` | pending |
| S8.2 | Silent reconnect with exponential backoff | `execution` | pending |
| S8.3 | Fail-closed: no new orders when the session has been down beyond a threshold | `execution` | pending — the hook already fails closed when `sherwood-server` is unreachable |
| S8.4 | Supersede logic for a replaced session | `execution` | pending |

The hook's HTTP surface (an axum route calling `HookGate::evaluate`), the agent-process
supervision, and the hook script that adapts `HookOutcome` to the CLI's permission schema all
land with S9 (`sherwood-server`).

## S9–S13 — ops shell

| Step | Task | Crate | Status |
|---|---|---|---|
| S9a | `sherwood-server` skeleton: axum on loopback (non-loopback bind refused), one `{code,message,correlation_id}` error envelope, bearer-token auth (constant-time, token generated into the vault on first run), `GET /v1/health`, `POST /v1/hook/pretooluse` wired to S7's `HookGate`. `sherwood serve <config>` with `[server]` + `[hook]` config | `server`, `cli` | done |
| S9b | RBAC roles (`viewer` / `operator` / `admin`), PAPER/LIVE toggle (admin + body re-auth, gated by `[server] allow_live`), kill-switch endpoint (admin + body re-auth), `GET /v1/control` | `server` | done |
| S9c | `GET /v1/metrics` (Prometheus text, hand-rolled — no `prometheus` crate), global fixed-window rate limit (`[server] rate_limit_per_min`), CORS for configured dashboard origins | `server` | done |
| S9d | Event feed: `GET /v1/events` **Server-Sent Events** (not a WebSocket — see the 2026-09-04 [decision log](DECISIONS.md)) streaming new audit-chain rows, consumed by the dashboard in place of polling `/v1/activity`. `serve` stays a control plane; it does not run the loop. | `server`, `frontend/` | done |
| S9e | `utoipa`-generated OpenAPI to replace the hand-written `docs/API.md` | `server` | deferred — annotation churn + 2 deps for a doc artifact; revisit once the route set stops moving |
| S11a | Read-only views over the persisted state: `GET /v1/portfolio`, `GET /v1/activity`, `GET /v1/audit/verify` (all viewer). `serve` opens the same `[general] state_path` `sherwood run` writes | `server`, `cli` | done — dashboard has real data to render before the full approval gate lands |
| S10 | Dashboard: React + Vite + TypeScript in `frontend/` — token login (session-only), status bar with the unmissable PAPER/**LIVE** badge, portfolio + activity + audit-integrity views, admin kill-switch and mode toggle (with re-auth). Strict build-time CSP. New CI job: `npm ci` · lint · typecheck · build | `frontend/` | done — config editor and the approvals queue are follow-ups (need a config API / S11) |
| S10.1 | `sherwood-server` serves the built dashboard (`[server] static_dir`) via `ServeDir` with SPA fallback + CSP / `X-Frame-Options` / `nosniff` / `Referrer-Policy` headers | `server`, `cli` | done |
| S11 | Approval gate: `pending → approved \| denied \| expired` state machine ([ADR-0005](adr/0005-approval-gate.md)), `auto` / `manual` modes, auto-deny timeout. In `manual` mode a risk-passing order creates a pending approval and the `PreToolUse` hook holds its response until the operator decides via `GET /v1/approvals` + `POST /v1/approvals/{id}`; dashboard `Approvals` card. `executed`/`settled` deferred to order reconciliation (S7.4). | `server`, `cli`, `frontend/` | done |
| S12a | **Per-session budgets** — `[server]` `max_session_orders` / `max_session_notional` / `max_session_duration_secs` hard stops (any `0` = unlimited). The `PreToolUse` hook denies every place-order once any cap latches, until `POST /v1/session/reset` (admin). `GET /v1/session` usage view; dashboard budget line + Reset. | `server`, `cli`, `frontend/` | done |
| S12b | Scheduler (`tokio-cron-scheduler`, timezone handling) and price-threshold monitors | `runtime` | deferred — both drive the *run loop*, which `serve` does not host (see the 2026-09-04 [decision log](DECISIONS.md)); revisit with a live feed (v0.2) or a scheduling front-end to `sherwood run` |
| S13 | Notifications and observability: OS notifications, audit feed UI with hash verification, Grafana dashboards, alert rules, log rotation | `runtime`, `server` |

## S14–S16 — hardening and release

| Step | Task | Status |
|---|---|---|
| S14a | `sherwood backtest <config>` — replay the `feed_path` CSV through the configured decider + risk gate (the same `run_loop`), print total return, max drawdown, closed-trade win rate, gross profit/loss, profit factor, expectancy. Deterministic; nothing persisted; `order_cooldown_secs` forced to `0`. See [BACKTEST.md](BACKTEST.md). | done |
| S14b | A/B comparison of two deciders in one run; a historical quote loader; walk-forward | pending |
| S15 | Reconnect and backoff stress tests, Docker image and compose stack, `sherwood backup` / `sherwood restore`, `RUNBOOK.md`, threat-model sign-off | pending |
| S16 | v0.1 release — signed tag, SBOM, release notes | pending |

## v0.2 — Solana modules

| Milestone | Crate |
|---|---|
| v0.2.1 | `sherwood-chain` — Solana RPC abstraction |
| v0.2.2 | `sherwood-signer` — custody tiers, signer isolation |
| v0.2.3 | `sherwood-wallets` — multi-wallet registry, per-strategy binding, spend ceilings |
| v0.2.4 | `sherwood-sniper` — pool event source, Geyser feed, live `RugScreen` |
| v0.2.5 | `sherwood-copytrade` — leader feed, swap decoding |
| v0.2.6 | `sherwood-router` — venue selection by notional and risk |

## Beyond v0.2

Postgres or TimescaleDB · NATS or Redis event backbone · HSM/KMS and MPC custody · real
multi-user RBAC · shadow mode (live logic, live data, no orders) · compliance tooling ·
non-LLM signal models · remote control app.
