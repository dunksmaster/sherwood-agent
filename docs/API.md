---
status: stub
last-updated: 2026-09-03
owner-step: S9
generated: true
---

# HTTP API

**Not yet written — and will not be hand-written.**

The API reference is generated from `utoipa` annotations on the axum handlers at **S9**. This
file will be replaced by generated output; edit the annotations, not this document.

## Contract (from [ENGINEERING-STANDARDS.md](ENGINEERING-STANDARDS.md#api))

- Versioned routes under `/v1/`. Breaking changes bump the prefix.
- One error envelope everywhere: `{ code, message, correlation_id }`.
- Bearer-token auth, constant-time comparison; RBAC roles `viewer` / `operator` / `admin`.
- Monetary values serialise as strings, never JSON numbers — `Decimal` precision must survive.
- The live-mode toggle and the kill switch require `admin` **and** re-authentication.
- No endpoint ever returns a secret.
