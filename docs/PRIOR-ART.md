---
status: accepted
last-updated: 2026-09-03
owner-step: S0
---

# Prior art

Every project consulted while designing this system, its licence, what was taken, and
explicitly whether any source code was used. Maintained per the clean-room policy in
[LICENSING.md](LICENSING.md).

**As of this entry, no source code from any project below has been copied into this
repository.** Everything implemented so far is original.

## Primary functional reference

### OpenTrade-Agent — Elastic Licence 2.0

A macOS desktop harness that runs `claude` / `codex` agents trading through the Robinhood
Agentic MCP. The closest existing system to v0.1's goal, and the reason
[ADR-0001](adr/0001-mcp-interaction-model.md) Option 3 is credible.

**Concepts studied:** fail-closed order-approval gate as a `PreToolUse` hook that long-polls
a local server · scheduler with cron plus event monitors · broker session lifecycle
(connect / reset / supersede, silent reconnect) · headless run budgets (turn limits,
run-duration deadlines) · settings system with validated bounds and live propagation · order
"cards" rendered from tool input · order-status reconciliation against the agentic order
ledger · strategy templates.

**Code used:** none. Licence is incompatible with this repository's dependency policy.
Every listed concept will be written as a specification in `docs/` and implemented from that
specification with the reference closed.

## Adaptable — permissive licences

| Project | Licence | Taken | Status |
|---|---|---|---|
| **barter-rs** | Apache-2.0 | Event-driven framework shape; plug-and-play `Strategy` and `RiskManager` trait seams; Tokio backbone | Studied. Adaptation permitted with attribution. None used yet. |
| **LEAN (QuantConnect)** | Apache-2.0 | Algorithm and data-handling API shape; multi-asset abstraction | Studied. None used yet. |
| **shadcn/ui** | MIT | Dashboard component kit | Planned direct use at S10 (copy-in component model) |
| **axum**, **tokio-cron-scheduler**, **notify-rust**, **sqlx**, **rust_decimal** | MIT / Apache-2.0 | Standard dependencies | Planned |

## Concept-only — non-permissive or unlicensed

Read for design understanding. No code, no assets, and — for unlicensed repositories — not
executed either.

| Project | Licence | Concept taken |
|---|---|---|
| **NautilusTrader** | LGPL-3.0 | Backtest and live sharing one code path; injected clock and RNG for determinism; order/position/account domain modelling |
| **AgentTrading** (Liu) | writeup | Hash-chained tamper-evident audit log; tier-based custody router; explicitly enumerated pre-trade risk checks |
| **AKIVA-AI/enterprise-crypto** | unverified | RBAC, audit, and separation-of-duties layered onto a trading system |
| **freqtrade** | GPL-3.0 | Strategy plugin API; backtest / dry-run / live parity; "protections" (cooldown, drawdown guard, stoploss guard); pairlist filters |
| **freqUI** | GPL-3.0 | Trading dashboard layout — positions, trade history, equity curve, bot-state controls |
| **OpenBB** | AGPL-3.0 | Data-provider abstraction: one interface, many backends |
| **hexnome/grpc-copy-trading-sniper-bot** | none stated | Multi-DEX coverage pattern over a gRPC/Geyser feed *(v0.2 relevance only)* |
| **trader-tony-v4** | none stated | Feature-completeness reference for a Rust Solana bot *(v0.2 relevance only)* |

> Repositories with no licence file are "all rights reserved". They may be read, but nothing
> may be copied or run. Several small Solana bot repositories in this space are low quality
> or actively malicious; none were executed.

## Deployed, not vendored

**Prometheus** and **Grafana** run as separate services. The application exposes a `/metrics`
endpoint and ships dashboard definitions as JSON. No metrics stack is built or embedded.

## Standards and documentation consulted

- Robinhood Agentic Trading documentation — the MCP endpoint, transport, OAuth flow, account
  requirements, and the constraint that agents may not transfer, stake, or lend.
- Model Context Protocol specification.
- Claude Code hooks documentation — the `PreToolUse` contract used by the approval gate.
- MADR — the ADR format used in [`adr/`](adr/).
- General HSM and key-management practice, for the v0.2 custody tiers.

## Maintenance

Add a row **before** implementing anything a project informed. Each entry states the licence
and whether code was used. If code is ever adapted from a permissive project, record the
specific files and add the required attribution to `NOTICE`.
