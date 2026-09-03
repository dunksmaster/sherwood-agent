---
status: stub
last-updated: 2026-09-03
owner-step: S15
---

# Runbook

**Not yet written.** Filled at **S15**, and reviewed as part of the threat-model sign-off.

An incident runbook that only exists in someone's head is not a control. This gets written
before v0.1 ships.

## Will cover

### Stopping

- Engaging the kill switch by all three routes — dashboard, `sherwood kill`, sentinel file
- What happens to in-flight and already-submitted orders
- Disengaging safely, and what to verify first

### Diagnosing

- Venue session will not connect or keeps dropping
- Local and Robinhood state disagree — running reconciliation
- Audit chain verification fails
- AI budget exhausted mid-run
- Repeated gate denials

### Recovering

- Database corruption — restoring from backup, then reconstructing recent state from the
  venue's order ledger
- Lost or rotated credentials
- Rolling back a bad release
- Post-incident: what to capture before restarting

### Routine

- Backup verification schedule
- Audit-chain verification schedule
- Dependency and advisory review cadence
