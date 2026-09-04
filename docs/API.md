---
status: partial
last-updated: 2026-09-04
owner-step: S9
generated: false
---

# HTTP API

The full reference will be generated from `utoipa` annotations at **S9c**; edit the
annotations, not this document, once that lands.

## Routes so far (S9a — `sherwood-server`)

| Method | Path | Auth | Notes |
|---|---|---|---|
| `GET` | `/v1/health` | none | `{ status, mode, uptime_secs }` — liveness |
| `POST` | `/v1/hook/pretooluse` | bearer | The `PreToolUse` order gate. Body: `{ tool_call: { name, arguments }, context: { portfolio, ref_price?, equity, unrealized_pnl, last_order_at? } }`. Returns `200` with `{ "decision": "allow" }` or `{ "decision": "deny", "reason": … }`. A denied tool call is **not** an HTTP error; only a malformed request is `4xx`. |

The server binds loopback only; a non-loopback bind is refused (TLS is a later concern). The
bearer token is generated into the `sherwood-secrets` vault on first `sherwood serve` and
compared in constant time.

## Contract (from [ENGINEERING-STANDARDS.md](ENGINEERING-STANDARDS.md#api))

- Versioned routes under `/v1/`. Breaking changes bump the prefix.
- One error envelope everywhere: `{ code, message, correlation_id }`.
- Bearer-token auth, constant-time comparison; RBAC roles `viewer` / `operator` / `admin`.
- Monetary values serialise as strings, never JSON numbers — `Decimal` precision must survive.
- The live-mode toggle and the kill switch require `admin` **and** re-authentication.
- No endpoint ever returns a secret.
