---
status: accepted
last-updated: 2026-09-03
owner-step: S0
---

# Documentation index

Every document carries front-matter with its `status`, `last-updated` date, and the build
step that owns it. A **stub** states its purpose and the step that will fill it — the gap is
recorded rather than forgotten.

## Conventions

- **One source of truth.** A fact lives in exactly one document; everything else links to it.
- **ADRs are immutable once accepted.** To change a decision, write a new ADR that supersedes
  the old one. Never edit an accepted ADR's Decision section.
- **Diagrams are inline Mermaid**, never images — they render on GitHub and diff as text.
- **Status values:** `draft` · `proposed` · `accepted` · `superseded` · `stub`.

## Plan and state

| Document | Status | What it covers |
|---|---|---|
| [CURRENT-STATE.md](CURRENT-STATE.md) | accepted | Honest audit of the scaffold: what is real, what is placeholder, known defects |
| [ROADMAP.md](ROADMAP.md) | accepted | Phases, steps S0–S16, MVP v0.1 definition, non-goals, v0.2 outline |
| [GRADE-TARGET.md](GRADE-TARGET.md) | accepted | The B (70+) licensing target, rubric dimensions mapped to roadmap steps |
| [DEFINITION-OF-DONE.md](DEFINITION-OF-DONE.md) | accepted | What "done" means per step — the rules that keep code from reading as scaffold |
| [ARCHITECTURE.md](ARCHITECTURE.md) | accepted | Crate map, data flow, event bus, risk-gate choke point, error taxonomy |
| [DECISIONS.md](DECISIONS.md) | accepted | ADR index and the running decision log |

## Decision records

| ADR | Status | Decision |
|---|---|---|
| [0001](adr/0001-mcp-interaction-model.md) | accepted | Agent harness with an in-line fail-closed risk gate (Option 3) |
| [0002](adr/0002-ai-decision-mode.md) | accepted | Advisory mode for v0.1; direct mode flag-gated, not live until baselined |
| [0003](adr/0003-storage-backend.md) | accepted | SQLite via `sqlx`, behind a `Store` trait |
| [0004](adr/0004-event-schema-versioning.md) | accepted | Every bus message carries a `version`; bump on observable change; no migration framework |

## Standards, security, safety

| Document | Status | What it covers |
|---|---|---|
| [ENGINEERING-STANDARDS.md](ENGINEERING-STANDARDS.md) | accepted | Lints, MSRV, error handling, determinism, testing, CI gates, process |
| [SECURITY.md](SECURITY.md) | accepted | Security architecture, credential lifecycle, spend controls, kill switch, reporting |
| [THREAT-MODEL.md](THREAT-MODEL.md) | draft | STRIDE across the pipeline, including MCP- and AI-specific threats |
| [AI-SAFETY.md](AI-SAFETY.md) | partially-implemented | Prompt injection, output schema enforcement, budgets, decision provenance |

## Legal and provenance

| Document | Status | What it covers |
|---|---|---|
| [LICENSING.md](LICENSING.md) | accepted | Licence strategy, dependency policy, what keeps commercial options open |
| [PRIOR-ART.md](PRIOR-ART.md) | accepted | Every project studied, its licence, and whether any code was used |
| [../CONTRIBUTING.md](../CONTRIBUTING.md) | accepted | Contribution process and CLA |

## Implementation

| Document | Status | What it covers |
|---|---|---|
| [DATA-MODEL.md](DATA-MODEL.md) | accepted | `sherwood-store` schema, migrations, the audit hash-chain, regenerating the sqlx offline cache |
| [RUNTIME.md](RUNTIME.md) | partial | Event bus (done, S3); supervisor / scheduler / approval state machine (pending, S3.4–3.5 / S11–S12) |

## Operations

| Document | Status | What it covers |
|---|---|---|
| [LIVE_EXECUTION.md](LIVE_EXECUTION.md) | accepted | The operator boundary and how live execution gets wired |
| [BACKTEST.md](BACKTEST.md) | partial | `sherwood backtest` — metrics and, deliberately, what it does not tell you |
| [OBSERVABILITY.md](OBSERVABILITY.md) | partial | `/v1/metrics` catalogue, `SHERWOOD_LOG_DIR` rotation, alert rules + Grafana JSON ([`deploy/`](../deploy/README.md)) |

## Stubs — written at the step that needs them

| Document | Filled at | Purpose |
|---|---|---|
| [ROBINHOOD-API.md](ROBINHOOD-API.md) | S7 | Asset classes, order types, settlement, rate limits, error codes |
| [API.md](API.md) | S9 | HTTP API — generated from `utoipa`, not hand-written |
| [FRONTEND-ARCH.md](FRONTEND-ARCH.md) | S10 | Dashboard state management, auth flow, reconnection |
| [DEPLOYMENT.md](DEPLOYMENT.md) | S15 | Docker, cross-compilation, install methods |
| [RUNBOOK.md](RUNBOOK.md) | S15 | Incident response, kill switch procedure, recovery |

## External reviews

| Document | Date | Verdict |
|---|---|---|
| [reviews/2026-09-03-kimi-master-plan.md](reviews/2026-09-03-kimi-master-plan.md) | 2026-09-03 | Partially adopted — strong code audit, wrong scope |
| [reviews/2026-09-03-plan-audit.md](reviews/2026-09-03-plan-audit.md) | 2026-09-03 | Largely adopted — caught the MCP-model blocker |
