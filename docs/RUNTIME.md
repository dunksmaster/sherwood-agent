---
status: stub
last-updated: 2026-09-03
owner-step: S3
---

# Runtime

**Not yet written.** Filled at **S3** (event bus and supervisor), extended at **S11–S12**
(approval gate, scheduler).

## Will cover

- **Event bus** — tokio `broadcast`, bounded at 1000, backpressure policy, the
  `BackpressureWarning` event, and why audit writes stay synchronous
- **Event schema and versioning** — every event carries `version: u16`; compatibility rules;
  the full catalogue (currently listed informally in
  [ARCHITECTURE.md](ARCHITECTURE.md#event-bus)). Becomes ADR-0004
- **Supervisor** — `start` / `stop` / `health_check`, config-driven component startup,
  restart policy
- **Scheduler** — cron expressions, timezone handling, event monitors
- **Approval gate state machine** — `proposed → pending → approved → executed → settled`,
  plus `denied` and `expired`; revocation window; timeout semantics. Becomes ADR-0005
- **Run budgets** — maximum orders, notional, and duration per run, with hard stops
- **Correlation id propagation** across the bus
