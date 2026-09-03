---
status: accepted
date: 2026-09-03
reviewer: external model
verdict: largely adopted
---

# Review — plan audit (external)

An audit of the consolidated Robinhood-first plan against engineering standards, the security
model, and the existing scaffold.

## Verdict

**Largely adopted — roughly 85%.** Correctly scoped to the Robinhood-only v0.1, and it caught
the single real blocker in the plan.

## Adopted

### The central finding

**The MCP interaction model was undefined.** The plan repeatedly said sherwood would "talk to
the Robinhood MCP" without specifying whether as a client, a server, or via a REST API. This
was correctly identified as blocking all of Phase 3. It is now
[ADR-0001](../adr/0001-mcp-interaction-model.md).

### Security and standards gaps

| Finding | Where it landed |
|---|---|
| OAuth token refresh and revocation strategy undefined | [SECURITY.md](../SECURITY.md#credentials) — a 401 is `Fatal`, never silently retried |
| Approval-gate state machine undefined | [RUNTIME.md](../RUNTIME.md), becomes ADR-0005 at S11 |
| **Prompt injection via market data** | [AI-SAFETY.md](../AI-SAFETY.md) — an entire document that would not otherwise exist |
| Model output schema enforcement and fallback chain | [AI-SAFETY.md](../AI-SAFETY.md#output-validation) |
| Token-budget circuit breaker | [AI-SAFETY.md](../AI-SAFETY.md#budgets-and-denial-of-service) |
| Decision provenance — log prompt and raw response | [AI-SAFETY.md](../AI-SAFETY.md#provenance) |
| **MCP tool allowlist** | S7.2; [SECURITY.md](../SECURITY.md#the-approval-hook) |
| Error taxonomy — retryable vs fatal | [ARCHITECTURE.md](../ARCHITECTURE.md#error-taxonomy) |
| Workspace lint manifest, pinned MSRV, feature flags | [ENGINEERING-STANDARDS.md](../ENGINEERING-STANDARDS.md) |
| Rate limiting, CORS, CSP on the local server | [SECURITY.md](../SECURITY.md#local-api) |
| Disaster recovery and backup key management | [RUNBOOK.md](../RUNBOOK.md) stub, S15 |
| Secrets zeroed on drop, minimal in-memory lifetime | [SECURITY.md](../SECURITY.md#credentials) |
| Pre-S0 repo bootstrap — branch protection, CODEOWNERS, `.deny.toml`, PR template, pre-commit | [ROADMAP.md](../ROADMAP.md#pre-s0--repo-bootstrap) |
| Robinhood-specific unknowns — asset classes, order types, fractional shares, **T+1 settlement affecting buying power**, rate limits, PDT rule | [ROBINHOOD-API.md](../ROBINHOOD-API.md) |
| Sub-step decomposition of the roadmap | [ROADMAP.md](../ROADMAP.md) |

## Corrected

| Audit claim | Correction |
|---|---|
| "Sherwood starts the Robinhood MCP server as a managed stdio subprocess" | Wrong. It is a **remote** OAuth-authenticated Streamable-HTTP server at `agent.robinhood.com/mcp/trading`. There is no local process to manage. Recorded in [ADR-0001](../adr/0001-mcp-interaction-model.md) so the mistake is not repeated |
| Options limited to A (MCP client) / B (MCP server) / C (REST) | Missed the model the primary reference actually uses — an **agent harness** that supervises a headless CLI agent holding the MCP connection. Added as Option 2, and Option 3 (harness + in-line fail-closed gate) is the current recommendation |

## Rejected or trimmed

| Item | Reason |
|---|---|
| ~25 documents before any code | Trimmed to 15 written now, 8 stubbed. `FRONTEND-ARCH.md` and `DEPLOYMENT.md` cannot be written meaningfully before the server exists; `EVENT-SCHEMA` folded into `RUNTIME.md`; `ERROR-TAXONOMY` folded into `ARCHITECTURE.md` |
| D1–D8 all treated as pre-S0 blockers | Only D1 (MCP model) and D2 (AI mode) genuinely gate S0. SQLx-vs-Diesel matters at S1, state management at S10, Docker base at S15. Recorded as standing questions instead |
| Certificate pinning for Robinhood | Pinning a third party's certificate you do not control converts a routine rotation into an outage. Standard `rustls` validation is correct. Recorded explicitly in [THREAT-MODEL.md](../THREAT-MODEL.md#spoofing) |
| Sharpe ratio in backtest metrics | Near-meaningless on these return distributions. Replaced with max drawdown, hit rate, profit factor, expectancy |
| Splitting `sherwood-runtime` into three crates | Premature. Modules within one crate until there is a reason to split |
| External anchoring of the audit chain as S1 design work | Kept as a no-op stub; real anchoring is a later concern |
