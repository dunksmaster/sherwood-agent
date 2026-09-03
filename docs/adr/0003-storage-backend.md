---
status: accepted
date: 2026-09-03
accepted: 2026-09-03
deciders: repository owner
owner-step: S0
---

# ADR-0003 — Storage backend

> **Accepted 2026-09-03: SQLite via `sqlx`, behind a `Store` trait.** The offline query
> cache (`.sqlx/`) is committed so CI needs no live database.

## Context

`Portfolio` is currently in-memory only; a crash loses all state
([CURRENT-STATE.md](../CURRENT-STATE.md), defect 1). v0.1 needs durable config, order and
fill history, position state, feed cursors, pending approvals, and an append-only audit log.

v0.1 is explicitly single-node and single-operator. Later milestones contemplate multi-node
operation and time-series retention at a volume SQLite would not serve well.

## Decision drivers

- Single-node, single-process, local-first for v0.1.
- Zero operational burden — no server to run alongside the binary.
- Compile-time confidence in queries.
- An append-only audit table with a hash chain, which needs ordinary transactional guarantees.
- A migration path to Postgres or TimescaleDB that does not require rewriting call sites.

## Options

| Option | Good | Bad |
|---|---|---|
| **SQLite via `sqlx`** | Embedded, no server; `sqlx` checks queries at compile time against a real schema; async; the same driver crate later speaks Postgres | Single-writer; not suited to high-rate time-series at scale |
| **SQLite via `rusqlite`** | Lighter, synchronous, very stable | Blocking API inside an async runtime; no compile-time query checking; different crate for any future Postgres move |
| **Diesel** | Mature ORM, strong migration tooling | Heavier; ORM abstraction is unnecessary for this schema; async story is weaker |
| **Postgres from day one** | No migration later; concurrent writers | Requires running a server for a single-user local application; contradicts local-first |

## Decision

**Proposed: SQLite via `sqlx`, behind a `Store` trait.**

All persistence goes through `Store`. No call site constructs SQL. An in-memory
implementation exists for tests; `SqliteStore` is the production implementation for v0.1.

Migrations are versioned and forward-only, applied at startup, with the schema version
recorded in the database.

## Consequences

- Adding Postgres later is a second `Store` implementation plus a connection-string switch —
  no changes above the trait. `sqlx` supports both, so query syntax stays largely portable.
- The single-writer constraint is acceptable while there is one process. It becomes a real
  limit the moment components are split across processes, which is a v0.2+ concern and is
  recorded in [ROADMAP.md](../ROADMAP.md).
- Time-series data (quotes, market snapshots) needs a retention policy from the start rather
  than unbounded growth — default 90 days, configurable, enforced by a periodic task.
- The audit table is append-only by convention and by trait design; there is no `Store` method
  that updates or deletes an audit row. SQLite cannot enforce that structurally, so it is
  additionally covered by the hash chain: any tampering breaks verification.
- `sqlx`'s compile-time checking requires either a live database or a committed
  `.sqlx/` offline query cache during CI. The offline cache will be committed.
