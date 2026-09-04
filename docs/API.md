---
status: partial
last-updated: 2026-09-04
owner-step: S9
generated: false
---

# HTTP API

The full reference will be generated from `utoipa` annotations at **S9c**; edit the
annotations, not this document, once that lands.

## Routes so far (S9a–S9c, S11a — `sherwood-server`)

| Method | Path | Min role | Notes |
|---|---|---|---|
| `GET` | `/v1/health` | none | `{ status, mode, kill_switch, uptime_secs }` |
| `GET` | `/v1/metrics` | none | Prometheus text exposition |
| `GET` | `/v1/control` | viewer | `{ mode, kill_switch }` |
| `GET` | `/v1/portfolio` | viewer | last persisted snapshot: `{ cash, realized_pnl, open_positions, positions[] }`. `404` if no `state_path` or no snapshot. |
| `GET` | `/v1/activity` | viewer | `?limit=N` (1–500, default 50). `{ recent: AuditEvent[], fills }`. |
| `GET` | `/v1/audit/verify` | viewer | `{ ok, entries }` or `{ ok: false, broken_at }`. |
| `POST` | `/v1/hook/pretooluse` | operator | The `PreToolUse` order gate. Body: `{ tool_call: { name, arguments }, context: { portfolio, ref_price?, equity, unrealized_pnl, last_order_at? } }`. Returns `200` with `{ "decision": "allow" }` or `{ "decision": "deny", "reason": … }`. A denied tool call is **not** an HTTP error; only a malformed request is `4xx`. |
| `POST` | `/v1/mode` | admin | `{ mode: "paper"\|"live", reauth: "<admin token>" }`. `live` is `403` unless `[server] allow_live = true`. |
| `POST` | `/v1/kill` | admin | `{ engage: bool, reauth: "<admin token>" }`. Engaging makes the hook deny every order immediately. |

Roles are assigned by which configured token authenticates: `token_ref` → admin, optional
`operator_token_ref` → operator, `viewer_token_ref` → viewer. The server binds loopback only
(a non-loopback bind is refused — TLS is a later concern). Tokens are generated into the
`sherwood-secrets` vault on first `sherwood serve` and compared in constant time.

A global fixed-window rate limit (`[server] rate_limit_per_min`, default 120) returns `429`
with the standard envelope when exceeded. CORS headers are emitted only for origins listed in
`[server] cors_origins`. `utoipa`-generated OpenAPI replaces this file at S9d.

## Contract (from [ENGINEERING-STANDARDS.md](ENGINEERING-STANDARDS.md#api))

- Versioned routes under `/v1/`. Breaking changes bump the prefix.
- One error envelope everywhere: `{ code, message, correlation_id }`.
- Bearer-token auth, constant-time comparison; RBAC roles `viewer` / `operator` / `admin`.
- Monetary values serialise as strings, never JSON numbers — `Decimal` precision must survive.
- The live-mode toggle and the kill switch require `admin` **and** re-authentication.
- No endpoint ever returns a secret.
