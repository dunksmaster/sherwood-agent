# sherwood dashboard

The local control-plane UI for `sherwood-server`. React + Vite + TypeScript, no
UI framework — a single stylesheet in the shadcn/ui idiom.

## Develop

```
npm install
npm run dev          # http://localhost:5173, proxies /v1 → 127.0.0.1:8787
```

Start the API alongside it:

```
cargo run -p sherwood-cli -- serve config.toml
```

Paste an API token when prompted (`sherwood secrets get api_token`). It is held
in `sessionStorage` — this tab only, never on disk.

## Build

```
npm run build        # → dist/, strict CSP injected at build time
npm run typecheck
npm run lint
```

In production `sherwood-server` serves `dist/` from its own origin (wiring is a
follow-up). See [`docs/FRONTEND-ARCH.md`](../docs/FRONTEND-ARCH.md).

## What it shows

- PAPER / **LIVE** badge (live is red and pulses), kill-switch state, uptime
- Portfolio: cash, realized P&L, open positions (from `GET /v1/portfolio`)
- Activity: audit-chain events streamed live over SSE (`GET /v1/events`) + fill
  count, with a chain-integrity badge
- Admin controls: kill switch and PAPER/LIVE toggle, each re-prompting for the
  admin token
