---
status: stub
last-updated: 2026-09-03
owner-step: S1
---

# Data model

**Not yet written.** Filled at **S1**, alongside the `sherwood-store` implementation.

## Will cover

- Table definitions: `portfolio_snapshots`, `fills`, `orders`, `audit_log`, `cursors`,
  `config_state`, `pending_approvals`
- The `Store` trait and its in-memory and SQLite implementations
- Migration strategy — forward-only, versioned, applied at startup
- **Audit hash chain**: row layout, `hash = sha256(prev_hash ‖ canonical_data)`, the
  verification command, and the external anchoring hook
- Retention policy — market snapshots default 90 days; fills and audit retained indefinitely
- `Decimal` storage as `TEXT`, never `REAL`

Decision recorded in [adr/0003-storage-backend.md](adr/0003-storage-backend.md).
