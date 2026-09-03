---
status: accepted
date: 2026-09-04
accepted: 2026-09-04
deciders: repository owner
owner-step: S3
---

# ADR-0004 — Event schema versioning

> **Accepted 2026-09-04:** every bus message carries a `version: u16`
> (`EVENT_SCHEMA_VERSION`, currently `1`). Bump it on any change to `Event` or
> `Envelope` a consumer could observe. No migration framework — a bump is a
> deliberate, reviewed change, and consumers may reject a version they do not
> understand.

## Context

The event bus ([RUNTIME.md](../RUNTIME.md)) decouples producers from consumers.
Once a second process, a persisted event stream, or an external subscriber
exists, a change to an event's shape that goes unnoticed becomes a silent
correctness bug: the consumer deserialises stale-shaped data, or misses a field,
and no one is told.

v0.1 is a single process, so nothing yet depends on this. The cost of adding it
now is one `u16` per message; the cost of retrofitting it once events are
persisted or cross a process boundary is a data migration.

## Decision drivers

- Cheap now, expensive later.
- A version mismatch should *fail loudly*, not corrupt silently.
- Do not build a migration framework before there is a v2 to migrate to.

## Options

| Option | Note |
|---|---|
| **Nothing** | Smallest now; silent breakage the first time an event shape changes under a persisted or remote consumer |
| **`version` field on the envelope** (chosen) | One `u16`; consumers can gate on it; no framework |
| Per-variant version | Finer-grained but there is no case that needs it yet |
| Full schema registry + migrations | Real infrastructure; premature with one process and four event types |

## Decision

The `Envelope` carries `version`. `EVENT_SCHEMA_VERSION` is a crate constant.

**Bump it** when any of these change in a consumer-visible way: a variant is
added, removed, or renamed; a field is added, removed, renamed, or retyped; the
envelope gains or loses a field.

**Do not bump** for internal changes a consumer cannot see (comments, the
`correlation_id` derivation for a variant that keeps the same id semantics,
`kind()` string stability aside).

A consumer that receives a version it does not understand should reject the
envelope and log it, not best-effort parse it.

## Consequences

- One field per message. Negligible.
- The bump is enforced by the PR review checklist, not by tooling — a reviewer
  asks "does this change event shape? did the version move?".
- When events are first persisted or sent between processes (v0.2+), the reader
  has a version to switch on, and a real migration path can be designed then
  against concrete v1→v2 needs rather than guessed at now.
- `docs/RUNTIME.md` carries the event catalogue and the bump rule; this ADR
  records *why* the field exists.
