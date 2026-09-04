---
status: partial
last-updated: 2026-09-04
owner-step: S10
---

# Frontend architecture

The local control-plane dashboard. Lives in [`frontend/`](../frontend/README.md).

## Stack

- **React 18 + Vite 5 + TypeScript**, `strict` with `noUnusedLocals` /
  `noUnusedParameters` / `noFallthroughCasesInSwitch`.
- **No UI framework.** A single hand-written `styles.css` in the shadcn/ui idiom
  (CSS variables, rounded panels, muted labels). shadcn's component _source_ is
  MIT and can be vendored later if the surface grows; for now the app is small
  enough that a stylesheet is less machinery than a component library.
- ESLint flat config (`@eslint/js` + `typescript-eslint` + `react-hooks`).
- No data-fetching library — a `usePoll` hook over `fetch` for the snapshot
  views, and a `fetch`-based SSE reader (`useAuditStream`) for the activity feed
  off `GET /v1/events`. `EventSource` isn't used because it can't send an
  `Authorization` header. SSE, not a WebSocket — the feed is one-directional.

State management is component-local `useState`; there is no store. If that stops
scaling it becomes ADR-0006 — not before.

## Serving

- **Dev:** `npm run dev` on `:5173`, with a Vite proxy sending `/v1` to
  `127.0.0.1:8787` so the browser is same-origin and needs no server-side CORS.
- **Production:** `npm run build` emits `dist/`; set `[server] static_dir =
  "frontend/dist"` and `sherwood-server` serves it at `/` from its own origin
  (SPA fallback to `index.html`), sending the CSP and hardening headers with
  every static response. `/v1/*` keeps precedence. Done (S10.1).

## Auth

The API bearer token is pasted into a prompt and held in `sessionStorage` —
this tab only, cleared on close, never written to disk. A `401` from any call
drops the token and returns to the prompt. RBAC is enforced server-side; the UI
simply surfaces a `403` inline (e.g. a viewer pressing the kill switch).

The kill switch and the PAPER/LIVE toggle re-prompt for the **admin token**,
which is sent as the request-body `reauth` field the server checks independently
of the session token.

## Money

`Decimal` values arrive as JSON **strings** (`rust_decimal` default). The client
never does arithmetic on them beyond `Number(...)` for display formatting
(`fmtMoney`), so precision is only ever lost in the rendered string, never in a
value sent back.

## PAPER / LIVE badge

Always visible in the status bar. `PAPER` is a quiet blue outline; `LIVE` is a
solid red fill that pulses. An engaged kill switch adds a second red badge.

## Content Security Policy

A strict CSP (`default-src 'self'`, `script-src 'self'`, no external origins) is
injected into `index.html` **at build time only** — the dev server needs inline
scripts for HMR. When `sherwood-server` serves `dist/` it also sends the same
CSP (plus `X-Frame-Options: DENY`, `X-Content-Type-Options: nosniff`,
`Referrer-Policy: no-referrer`) as response headers; the string is kept in sync
between `frontend/vite.config.ts` and `sherwood_server::DASHBOARD_CSP`.

## Views

| View | Source | Status |
|---|---|---|
| Status bar — mode, kill switch, uptime | `GET /v1/health` | done |
| Portfolio — cash, realized P&L, positions | `GET /v1/portfolio` | done |
| Activity — audit events (live via SSE), fill count, chain-integrity badge | `GET /v1/events`, `GET /v1/activity`, `GET /v1/audit/verify` | done |
| Controls — kill switch, PAPER/LIVE toggle | `POST /v1/kill`, `POST /v1/mode` | done |
| Approvals — pending order cards with Approve / Deny | `GET /v1/approvals`, `POST /v1/approvals/{id}` | done |
| Config editor | — | pending (needs a config API) |
| Charts (P&L curve) | — | pending (library choice deferred until there is data to plot) |
