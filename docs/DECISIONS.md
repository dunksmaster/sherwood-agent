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
| [0001](adr/0001-mcp-interaction-model.md) | MCP interaction model | **proposed** | — |
| [0002](adr/0002-ai-decision-mode.md) | AI decision mode | **proposed** | — |
| [0003](adr/0003-storage-backend.md) | Storage backend | **proposed** | — |

Planned, to be written when the step that needs them arrives:

| ADR | Title | Written at |
|---|---|---|
| 0004 | Event schema versioning | S3 |
| 0005 | Approval gate state machine | S11 |
| 0006 | Frontend state management | S10 |
| 0007 | Deployment and packaging | S15 |

## Decision log

Decisions that shaped the project but do not each warrant a full ADR. Newest first.

### 2026-09-03

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
| Q2 | Where does it run — local Windows, Linux VPS, cloud? | S15 (packaging) | Local first, Docker for portability |
| Q3 | Capital scale being designed around | S5 (spend caps) | Small — caps default low |
| Q4 | NVIDIA NIM model choice | S4 | Undecided |
| Q5 | Daily AI spend ceiling | S4 | Undecided |
| Q6 | Historical data source for backtest | S14 | Undecided |
| Q7 | Repository visibility | Pre-S0 | Public, private recommended |
| Q8 | `hoodmap` planning pass — now or later? | — | Parked |
