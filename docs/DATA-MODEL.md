---
status: accepted
last-updated: 2026-09-04
owner-step: S1
---

# Data model

`sherwood-store` — durable state on SQLite via `sqlx`, behind the
[`Store`](../crates/store/src/lib.rs) trait. Decision recorded in
[adr/0003-storage-backend.md](adr/0003-storage-backend.md).

## Conventions

- **Timestamps** are RFC 3339 UTC strings (`TEXT`).
- **Money and quantities** are decimal strings (`TEXT`), parsed with
  `rust_decimal::Decimal`. Never `REAL` — a float column would silently lose
  precision that the rest of the system is careful to keep.
- **Migrations** are forward-only, in `crates/store/migrations/`, embedded with
  `sqlx::migrate!` and applied on `SqliteStore::open`.
- **Compile-time-checked queries.** Every `sqlx::query!` is verified against the
  schema at build time. CI has no database — it uses the committed `.sqlx/`
  offline cache (`SQLX_OFFLINE=true`). Regenerate after changing any query:

  ```bash
  export DATABASE_URL="sqlite:crates/store/.sqlx-dev.db"
  sqlx database create && sqlx migrate run --source crates/store/migrations
  cargo sqlx prepare --workspace
  ```

## Tables (migration `0001_init`)

Only what v0.1 uses. `config_state`, `cursors`, and `pending_approvals` are
added by the migration that accompanies the step that first writes them (S2, S5,
S11 respectively) — an unused table is scaffold
([DEFINITION-OF-DONE.md](DEFINITION-OF-DONE.md)).

### `portfolio_snapshots`

| column | type | notes |
|---|---|---|
| `id` | INTEGER PK | autoincrement |
| `taken_at` | TEXT | RFC 3339 |
| `state_json` | TEXT | the whole `Portfolio`, serialised |

The **latest** snapshot is the authoritative balance on restart. History is kept
for inspection, not replay.

### `fills`

| column | type | notes |
|---|---|---|
| `id` | INTEGER PK | autoincrement, defines order |
| `order_id` | TEXT | client order id |
| `symbol` | TEXT | indexed |
| `address` | TEXT | nullable — set for on-chain assets (v0.2) |
| `side` | TEXT | `buy` \| `sell` (CHECK) |
| `qty`, `price`, `fee` | TEXT | decimal strings |
| `venue` | TEXT | serialised `Venue` (`paper`, …) |
| `filled_at` | TEXT | when the venue filled it; indexed |
| `recorded_at` | TEXT | when the store wrote the row |

### `audit_log` — tamper-evident hash chain

| column | type | notes |
|---|---|---|
| `seq` | INTEGER PK | 1-based, contiguous, assigned by the store |
| `at` | TEXT | RFC 3339 |
| `kind` | TEXT | e.g. `fill`, `gate_reject`, `run_end` |
| `data_json` | TEXT | canonical (key-sorted) JSON payload |
| `prev_hash` | TEXT | hex SHA-256 of the previous row's `hash`; genesis = 64 zeros |
| `hash` | TEXT | `hex(sha256(prev_hash ‖ "\n" ‖ seq ‖ "\n" ‖ at ‖ "\n" ‖ kind ‖ "\n" ‖ data_json))` |

There is **no `UPDATE` or `DELETE` path** for `audit_log` in the `Store` trait.
Editing or deleting any row changes its `hash` (or breaks `seq` contiguity), and
[`verify_audit_chain`](../crates/store/src/lib.rs) walks the whole chain from
genesis and reports the first `seq` where the recomputed hash, the stored hash,
or the `prev_hash` link disagrees.

`serde_json::Value` orders object keys, so `serde_json::to_string` of a payload
is canonical for a given value — the same payload always hashes the same way.

External anchoring of the chain head (writing the latest `hash` somewhere
outside the database on a schedule) is a later hardening task; the stub is noted
in the roadmap.

## What is not persisted yet

Config (S2 → `config_state`), feed cursors (S5 → `cursors`), and pending
approvals (S11 → `pending_approvals`). The runner currently resumes only the
portfolio snapshot; resuming mid-feed needs a real feed first (S5).
