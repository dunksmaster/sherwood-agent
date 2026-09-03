---
status: stub
last-updated: 2026-09-03
owner-step: S10
---

# Frontend architecture

**Not yet written.** Filled at **S10** — it cannot be specified meaningfully before the
server's API exists at S9.

## Will cover

- Stack: React + Vite + TypeScript + shadcn/ui (MIT), served as static files by
  `sherwood-server`
- State management choice and rationale — becomes ADR-0006
- Auth flow: local bearer token, where it is entered, how it is held
- WebSocket lifecycle: subscription model, reconnection with backoff, missed-event recovery
- Views: Config · Portfolio · Activity · Approvals · Settings
- **PAPER / LIVE badge** — unmissable, and visibly different in live mode
- Kill-switch control: admin-only, confirmation dialog
- Charting library choice and the money-formatting rules (`Decimal` arrives as strings)
- Content Security Policy — strict, no external script or style origins
