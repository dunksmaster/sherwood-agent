---
status: partial
last-updated: 2026-09-04
owner-step: S3
---

# Runtime

The internal wiring: the event bus (done, S3), and — still to come — the
supervisor, scheduler, and approval state machine (S3.4–3.5, S11–S12).

## Event bus — `sherwood-events`

Components publish [`Event`](../crates/events/src/lib.rs)s and never call each
other directly. Adding a strategy, a metrics sink, or a notifier is a new
subscriber, not a change to any producer.

### Transport

A `tokio::sync::broadcast` channel, capacity **1000**. Each subscriber holds its
own receiver. `Bus` is `Clone` — every clone is another publisher, and the
channel stays open until the last `Bus` handle is dropped. Publishing with no
subscribers is a no-op, not an error.

### Envelope and versioning

Every event is wrapped:

```rust
struct Envelope {
    version: u16,          // EVENT_SCHEMA_VERSION, currently 1
    at: DateTime<Utc>,
    correlation_id: String,// order id, or tick-N / run-LABEL
    event: Event,
}
```

`EVENT_SCHEMA_VERSION` is bumped whenever the shape of `Event` or `Envelope`
changes in a way a consumer could observe. A consumer may reject an envelope
whose version it does not understand. There is no migration framework — a
version bump is a deliberate, reviewed change. Recorded in
[adr/0004-event-schema-versioning.md](adr/0004-event-schema-versioning.md).

### Event catalogue (v1)

A variant exists only when it has a real emitter **and** a real consumer.

| Variant | Emitted by | Consumed by |
|---|---|---|
| `Decided { tick, price, decision }` | run loop, on a non-`Hold` verdict | store (→ `decision` audit row), tracing |
| `OrderFilled(Fill)` | run loop, after `Portfolio::apply` | store (→ `fills` row + `fill` audit row), tracing |
| `RiskRejected { order_id, symbol, reason }` | run loop, when the gate refuses | store (→ `gate_reject` audit row), tracing |
| `RunEnded { label, interrupted, cash, realized_pnl }` | run loop, on exit | store (→ `run_end` audit row), tracing |

Executor errors are logged, not evented — a real venue's rejections arrive
through the [error taxonomy](ARCHITECTURE.md#error-taxonomy) at S7.

### Backpressure

The channel is bounded. A subscriber that falls more than 1000 events behind
loses the oldest ones and `run_subscriber` logs
`subscriber fell behind; events were dropped` with the count. A handler that
returns an error is logged and the loop continues. **A subscriber can never take
the bus, or another subscriber, down.**

The portfolio *snapshot* is the one thing the run loop writes to the store
directly rather than through the bus — the loop owns that state. Everything a
*separate* component needs to observe goes through the bus.

### Subscribers

- `run_subscriber(rx, sub)` drives a `Subscriber` until the bus closes, then
  returns — so a shutdown path can `await` it to flush.
- `TracingSubscriber` — one structured `sherwood::events` log line per event.
  Always attached. The observability step (S13) scrapes this.
- `StoreSubscriber` (in `sherwood-store`) — persists fills and audit rows.
  Attached whenever a `state_path` is configured.

## Supervisor — pending (S3.4–3.5)

`start` / `stop` / `health_check`, config-driven component startup. Deferred
until there are multiple long-lived components to supervise (S4+: the decider
registry and, later, live feeds). Building it now, against a run loop that
finishes in milliseconds, would be scaffold.

## Scheduler and approval state machine — pending (S11–S12)

Cron + event monitors; the `proposed → pending → approved → executed → settled`
approval machine (which becomes ADR-0005). Filled at those steps.
