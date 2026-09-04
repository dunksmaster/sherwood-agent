---
status: accepted
last-updated: 2026-09-03
owner-step: S0
---

# Decisions

## ADR index

Architecture Decision Records live in [`adr/`](adr/). They use MADR format and are
**immutable once accepted** — to change a decision, write a new ADR that supersedes the old
one rather than editing it.

| ADR | Title | Status | Supersedes |
|---|---|---|---|
| [0001](adr/0001-mcp-interaction-model.md) | MCP interaction model | accepted 2026-09-03 | — |
| [0002](adr/0002-ai-decision-mode.md) | AI decision mode | accepted 2026-09-03 | — |
| [0003](adr/0003-storage-backend.md) | Storage backend | accepted 2026-09-03 | — |
| [0004](adr/0004-event-schema-versioning.md) | Event schema versioning | accepted 2026-09-04 | — |
| [0005](adr/0005-approval-gate.md) | Approval gate state machine | accepted 2026-09-04 | — |

Planned, to be written when the step that needs them arrives:

| ADR | Title | Written at |
|---|---|---|
| 0006 | Frontend state management | S10 (deferred — component-local `useState`, no store yet; see [FRONTEND-ARCH.md](FRONTEND-ARCH.md)) |
| 0007 | Deployment and packaging | S15 |

## Decision log

Decisions that shaped the project but do not each warrant a full ADR. Newest first.

### 2026-09-04

**No generated OpenAPI for v0.1 (S9e closed).** `docs/API.md` is the maintained
API contract. `utoipa` was evaluated and declined: it needs ~20 `ToSchema`
derives and ~15 `#[utoipa::path]` attributes across eight files, plus
`value_type` overrides for foreign types (`Decimal`, `AuditEvent`,
`HookOutcome`, `Portfolio`), and it introduces annotation-vs-code drift with no
CI guard. There is no consumer — the API is loopback-only, the dashboard uses a
hand-written typed client, and there is no Swagger UI. Revisit if a third-party
API consumer appears.

**v0.1 scope met.** S0–S15 are done for a **paper** release. Everything still on
the roadmap (S7.4 reconciliation, S8 reconnection, S12b scheduler/monitors,
S14b A/B backtest, S13b OS notifications) is gated on a live Robinhood MCP
connection or is explicitly v0.2. The `v0.1.0` tag is the operator's to cut.

**Threat model reviewed and signed off (S15)** against the implemented paper system.
`docs/THREAT-MODEL.md` status → `reviewed`; its sign-off section splits mitigations into
implemented / partial / deferred (v0.2 / S7.4–S8). No blocking gap for a paper-only v0.1.
Standing question **Q2** (where it runs) resolved: local first, Docker for portability — the
server stays loopback-only, so there is no network-exposure recipe.

**`sherwood serve` is a pure control plane — it does not run the trading loop.** The paper
loop stays in `sherwood run` (feed → decider → gate → paper executor → store); `serve`
reads the state `run` persists and controls the risk gate / kill switch. Rationale: under
[ADR-0001](adr/0001-mcp-interaction-model.md) Option 3 the live trading in v0.1 is done by an
external agent through the `PreToolUse` hook, not our in-process loop; and a long-running
`serve` that also ran the loop would need a non-terminating feed, which is a v0.2 live-feed
concern. Folding the loop in-process remains possible later without an API change.

**The event feed is Server-Sent Events, not a WebSocket.** `GET /v1/events` streams new
audit-chain rows. SSE fits a one-directional read-only feed, rides the existing bearer-auth
middleware with no upgrade handshake, and browsers reconnect it natively. The roadmap's
"WebSocket" wording is superseded here. A WebSocket can still be added later if a
bidirectional need appears.

### 2026-09-03

**ADR-0001, 0002, 0003 accepted.** MCP model: agent harness with an in-line fail-closed risk
gate (Option 3). AI mode: advisory for v0.1. Storage: SQLite via `sqlx`. S0's blocking
decision is closed; Phase 3 is unblocked in principle, pending the S7 research items listed
in ADR-0001.

**Repository visibility: public.** Standing question Q7 resolved. Superseded the same-day
choice of "private": the owner elected to develop this as an open-source project shared with
the community. The security implications are accepted and compensated for in
[SECURITY.md](SECURITY.md#disclosure-posture) — the controls do not depend on the design
being secret, no secret is or was committed, and `main` is branch-protected. Going public
also unblocks free branch protection and rulesets (P0.2), which require GitHub Pro on a
private repo.

**Trade recorded against the licensing goal:** a public MIT repository can be used by anyone
without payment. This narrows, though the owner holding all copyright (plus the CLA) keeps a
future dual-licence option technically open. See [LICENSING.md](LICENSING.md#current-licence-and-the-open-question).

**Grade target: B (70+).** The repository is presented for licensing against a published
rubric; B licenses reliably. The gap is mostly code substance and process, not docs — the
grader reads code and flags padding. Mapped dimension-by-dimension to roadmap steps in
[GRADE-TARGET.md](GRADE-TARGET.md); the cheap hygiene items (H1–H9) are folded into Pre-S0.
*Why recorded:* so future work is weighed against a concrete bar, and so it is explicit that
no shortcut substitutes for building S1–S5.

**Scope: v0.1 is Robinhood-only.** No Solana, no wallets, no private keys. Solana becomes
v0.2, additive, in the same repository behind the same traits.
*Why:* the operator wants automated trading working sooner; removing key custody removes the
largest source of both complexity and risk. A sniper is impossible through the Robinhood MCP
(no pools, no mempool, no wallet), so the two capabilities were separated by milestone rather
than compromised into one.

**One repository, not two.** Robinhood and Solana share roughly 70% of the code — types, risk
gate, portfolio, decision layer, server, dashboard, approvals, scheduler, audit. Only venue
adapters differ, and those are already behind the `Executor` trait.
*Revisit if:* the two are ever to be licensed or sold as separate products.

**Clean-room method for all borrowed concepts.** Design ideas may be taken from any project;
source code may not, unless the licence is permissive. Implementation happens from a written
functional spec, not with the reference repository open.
*Why:* provenance survives IP due diligence, which is a prerequisite for any future licensing
of this codebase. See [LICENSING.md](LICENSING.md) and [PRIOR-ART.md](PRIOR-ART.md).

**Permissive dependencies only.** MIT, Apache-2.0, BSD, ISC, Zlib. No GPL, AGPL, LGPL, ELv2,
BSL, or non-commercial licences anywhere in the tree. Enforced by `cargo-deny` from S0.
*Why:* a single copyleft dependency forecloses commercial licensing of the whole work.

**Observability is deployed, not vendored.** Prometheus and Grafana run as separate services;
the application exposes `/metrics` and ships dashboard JSON. We do not build a metrics stack.

**No code until S0 is complete.** Standards, threat model, CI gates, and the provenance trail
land before the first implementation commit.
*Why:* retrofitting lint policy, licence hygiene, and audit design costs far more than
writing them first.

**Documentation lives in the repository.** Markdown under `docs/`, versioned with the code,
reviewed via PR. Published canvases are rendered views, never the source of truth.

**Roadmap step count is 16 plus a pre-step.** Pre-S0 repo bootstrap, S0 governance, S1–S16 as
listed in [ROADMAP.md](ROADMAP.md).

## Standing questions

Not blocking the current step, but they need answers before the step named.

| # | Question | Needed by | Current default |
|---|---|---|---|
| Q1 | Single operator, or plan for multiple from the start? | S9 (RBAC depth) | Single operator, roles wired anyway |
| Q2 | ~~Where does it run?~~ | S15 | **Resolved (2026-09-04):** local first (bare binary + systemd), Docker for portability; loopback-only, no network exposure |
| Q3 | Capital scale being designed around | S5 (spend caps) | Small — caps default low |
| Q4 | NVIDIA NIM model choice | S4 | Undecided |
| Q5 | Daily AI spend ceiling | S4 | Undecided |
| Q6 | Historical data source for backtest | S14 | Undecided |
| Q7 | ~~Repository visibility~~ | Pre-S0 | **Resolved: public** (2026-09-04) — open-source project |
| Q8 | `hoodmap` planning pass — now or later? | — | Parked |
