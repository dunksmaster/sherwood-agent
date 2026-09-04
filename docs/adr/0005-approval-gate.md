---
status: accepted
date: 2026-09-04
accepted: 2026-09-04
deciders: repository owner
owner-step: S11
---

# ADR-0005 — Approval gate state machine

> **Accepted 2026-09-04:** the approval gate sits between "the risk gate would
> allow this order" and "the agent may place it". Two modes — `auto`
> (transparent) and `manual` (every risk-passing order waits for the operator).
> v0.1 models `pending → approved | denied | expired`; `executed` and `settled`
> are deferred until fills can be reconciled against the venue (S7.4). The
> waiting `PreToolUse` hook holds its HTTP response until the decision or a
> timeout; a timeout auto-denies.

## Context

Under [ADR-0001](0001-mcp-interaction-model.md) Option 3 the `PreToolUse` hook
answers allow/deny for every order the trading agent tries to place. So far that
answer is purely the `RiskGate` verdict. For a funded account the operator will
often want a human confirmation on the actual order — the size, the symbol, the
moment — that a static risk config cannot express.

This needs a small state machine, a queue the dashboard can render, and a way to
make the synchronous hook call wait for an asynchronous human.

## Decision drivers

- **The hook stays fail-closed.** Anything other than an explicit approval —
  timeout, server restart, ambiguity — must resolve to deny.
- **`auto` must remain zero-overhead.** The common case (rules-only, no human)
  pays nothing.
- **No new persistence for v0.1.** Approvals are in-memory; a lost approval on
  restart is a deny, which is safe.
- **Only real states.** `executed`/`settled` require observing the fill, which
  is not wired until the live MCP and reconciliation (S7.4) exist. Modelling
  them now would be fiction.

## Decision

### States

```
          approve
pending ───────────► approved
   │
   │ deny
   ├───────────────► denied
   │
   │ timeout / sweep
   └───────────────► expired
```

`approved`, `denied`, `expired` are terminal. A decision on a terminal approval
is a no-op (`404`-style "unknown or already decided").

`executed` (the agent placed the approved order) and `settled` (the venue
filled it) are **documented but not implemented** — they arrive with order
reconciliation.

### Modes

| Mode | Behaviour |
|---|---|
| `auto` (default) | The gate is transparent. A risk-passing order is allowed immediately, exactly as before this ADR. |
| `manual` | A risk-passing **place-order** call creates a `pending` approval. The hook call blocks until the operator decides or `approval_timeout_secs` (default 60) elapses. Reads and cancels are never held. |

### Mechanism

- `ApprovalStore` holds entries in memory. Each carries a `tokio::sync::watch`
  channel; the waiting hook awaits a non-`pending` value, bounded by
  `tokio::time::timeout`.
- A 5-second sweeper task marks stale `pending` entries `expired` even when no
  hook is waiting (the agent may have abandoned its request), so the queue and
  any waiter never disagree for long.
- Terminal entries are retained (capped at 50) so the dashboard shows recent
  history.

### API

| Method | Path | Role | |
|---|---|---|---|
| `GET` | `/v1/approvals` | viewer | `{ mode, pending, approvals[] }` |
| `POST` | `/v1/approvals/{id}` | operator | `{ decision: "approve" \| "deny", reason? }` |

## Consequences

- The `PreToolUse` hook's own timeout, and the agent CLI's hook timeout, must
  both exceed `approval_timeout_secs`, or a slow-but-valid approval is turned
  into a transport error instead of a clean deny. This is called out in
  [SECURITY.md](../SECURITY.md#the-approval-hook).
- Approvals do not survive a server restart. A pending order at restart becomes
  an implicit deny (the hook connection drops). Acceptable for v0.1;
  persistence is revisited if it becomes a real operational problem.
- `manual` mode makes the hook a long-lived request. It counts once against the
  rate limiter and is recorded once by the metrics middleware (status is fixed
  when headers flush).
- When order reconciliation lands, `executed`/`settled` extend this machine
  without changing the `pending → decision` core or the API.

## Alternatives considered

- **Persist approvals to the store.** Deferred — adds a table and a migration
  for a queue that is empty most of the time and whose loss is safe.
- **Push approvals over the SSE feed instead of polling.** The dashboard polls
  `/v1/approvals` on a short interval; approvals are rare and the operator is
  watching. Pushing them is a later refinement, not a v0.1 need.
- **A separate approving service.** Overkill for a single-operator loopback
  tool.
