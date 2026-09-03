---
status: accepted
date: 2026-09-03
accepted: 2026-09-03
deciders: repository owner
owner-step: S0
---

# ADR-0002 — AI decision mode

> **Accepted 2026-09-03: Mode A (advisory) for v0.1.** Mode B (direct) remains available
> behind a config flag that the validator refuses in live mode until a paper baseline exists.
> Under [ADR-0001](0001-mcp-interaction-model.md) Option 3 this governs only the supplementary
> `sherwood-decision` path in v0.1; it becomes the primary path if a self-owned decision
> engine is added later.

## Context

The system includes a language model in the trading loop. There are two materially different
ways to place it, and they carry different failure modes.

This decision partly depends on [ADR-0001](0001-mcp-interaction-model.md): under Option 3 of
that ADR the deciding agent is a general-purpose coding agent, and this ADR governs only the
supplementary `sherwood-decision` path. Under Option 1 this ADR governs the primary path.

## Decision drivers

- A model can hallucinate a symbol, a side, or a quantity.
- Market data is attacker-influenced text. Token names, descriptions, and news snippets are
  a prompt-injection surface — see [AI-SAFETY.md](../AI-SAFETY.md).
- Deterministic rules can be backtested and property-tested. Model output cannot.
- The operator has expressed interest in an NVIDIA-driven decision layer, which implies
  wanting the model to do real work rather than annotate.

## Options

### Mode A — advisory

The model produces structured *annotations*: sentiment, a confidence score, flagged risks,
a short rationale. `RuleDecider` reads those as additional inputs and makes the actual
buy/sell/hold call using deterministic thresholds.

- **Good:** rules stay in control and remain backtestable; a hallucination degrades signal
  quality but cannot itself produce an order; the blast radius of prompt injection is a
  wrong number in a threshold comparison.
- **Bad:** limits what the model contributes; more work to design a useful annotation schema
  than to just ask for a decision.

### Mode B — direct

The model returns `Decision::Buy | Sell | Hold` with a size fraction and a reason.
`RiskGate` is the only thing between it and an order.

- **Good:** the model does the reasoning it is good at; simpler prompt; matches what the
  operator described wanting.
- **Bad:** a hallucination *is* an order proposal; prompt injection can attempt to steer a
  trade directly; nothing in the path is backtestable; the gate becomes the sole backstop and
  must be assumed to be the only thing working.

## Decision

**Proposed: Mode A for v0.1, with Mode B available behind an explicit configuration flag that
is refused while `mode = "live"` until Mode A has been run in paper long enough to establish
a baseline.**

Both modes share the same output-schema enforcement, token budget, and provenance logging
from [AI-SAFETY.md](../AI-SAFETY.md). The difference is only whether the parsed output
becomes a `Decision` directly or an input to `RuleDecider`.

## Consequences

- `sherwood-decision` needs two output schemas — an annotation schema and a decision schema —
  and the registry selects by configured mode.
- The `RuleDecider` gains optional annotation inputs, which must default to neutral so that
  the rules behave identically when no model is configured.
- Mode B being live-gated means the config validator must reject the combination of
  `ai.mode = "direct"` and `general.mode = "live"` unless an explicit override flag is set,
  and that override must be recorded in the audit log.
- Backtests (S14) exercise the rules and, in Mode A, replayed annotations. They can never
  exercise Mode B faithfully. The backtest report must say so.
