---
status: accepted
date: 2026-09-03
reviewer: external model (Kimi)
verdict: partially adopted
---

# Review — "Master Plan v0.1" (external)

An externally produced master plan for the project, reviewed and partially folded in.

## Verdict

**Partially adopted.** A strong *code audit*, a poor *roadmap* for the current scope.

The plan was written against the pre-pivot scope: Solana-first, with `sherwood-wallets`, key
custody tiers, HSM, signer isolation, and questions about DEX venues and Geyser providers.
Roughly 40% of it addressed work that had already been deferred to v0.2. It contained **no
Robinhood-specific work at all** — no MCP adapter, no OAuth or session lifecycle, no order
reconciliation, no approval gate, no server or dashboard — which are precisely the v0.1
deliverables.

## Adopted

| Finding | Where it landed |
|---|---|
| **Current-state gap table** — a genuine audit of the scaffold, verified against source | [CURRENT-STATE.md](../CURRENT-STATE.md), essentially wholesale |
| `Portfolio` is in-memory; a crash loses all state | Defect 1; addressed at S1 |
| `runner.rs` hardcodes `Asset::symbol("ROAR")` | Defect 3; addressed at S5.2 |
| `synthetic_series()` is a hardcoded price path | Defect 2; addressed at S5.1 |
| No graceful shutdown on SIGINT | Defect 9; addressed at S5.6 |
| `max_position_fraction` can be negative or > 1 — no validation | Defect 10; addressed at S2.1 |
| **`RiskGate` ignores unrealized P&L** — an open position can bleed while the gate still admits entries | Defect 8; addressed at S5.4 |
| No cooldown or max-open-positions cap | Defect 12; addressed at S5.4 |
| No retry or circuit breaker on the executor | Defect 11 |
| Concrete `Store` trait sketch | Informed [adr/0003](../adr/0003-storage-backend.md) and the S1 task list |
| Event-type catalogue | Informed [ARCHITECTURE.md](../ARCHITECTURE.md#event-bus) |
| Per-crate coverage targets (80 / 70 / 60%) | [ENGINEERING-STANDARDS.md](../ENGINEERING-STANDARDS.md#testing) |
| "No code until Phase 0 complete" | Adopted as the S0 exit criterion |

## Rejected or deferred

| Item | Reason |
|---|---|
| Solana-first sequencing | Contradicts the locked v0.1 scope. Moved to the v0.2 outline |
| `sherwood-wallets` and custody tiers in Phase 1 | v0.1 holds no keys — Robinhood is OAuth. Deferred to v0.2 |
| Key-custody-centric STRIDE table | Rewritten around OAuth, the agent trust grant, and prompt injection |
| Governance folded into prose rather than scheduled | Made an explicit S0 phase with its own exit criteria |
| Ops shell absent from the roadmap | Added as S9–S13; it is most of the product |
| "Current licence MIT, evaluate dual-licence later" | [LICENSING.md](../LICENSING.md) treats it as an open decision now, because clean-room and CLA must start before implementation, not after |
| `AiDecider`'s closure wrapper listed as a gap | It is a deliberate seam, not a defect |

## Note

The gap table was independently verified against the source before being adopted. Every
defect listed in `CURRENT-STATE.md` was confirmed to exist.
