# sherwood-agent v0.1.0

First feature-complete release. **Paper trading only** — there is no code path to
a live venue in this version.

## What it is

A local control plane for agentic trading. Every order — whoever proposes it, a
rule, a language model, or an external agent through the `PreToolUse` hook —
passes the same checks before it could reach a venue:

1. **`RiskGate`** — kill switch, realized daily-loss breaker (hard stops); plus
   notional / position-fraction / unrealized-loss / open-position / per-symbol
   cooldown limits on new exposure.
2. **Approval gate** — in `manual` mode a human approves or denies each
   risk-passing order (auto-deny on timeout).
3. **Per-session budget** — order-count / notional / duration hard stops that
   latch a deny until an admin resets.

Everything it does is recorded in a **hash-chained, tamper-evident audit log**
you can verify (`GET /v1/audit/verify`, and a live badge in the dashboard).

## Components

| | |
|---|---|
| **Engine** | deterministic multi-asset run loop; `rust_decimal` throughout; injected `Clock`; CSV price replay |
| **Deciders** | `RuleDecider` (momentum + TP/SL); `AiDecider` — OpenAI-compatible provider, `<market_data>` injection guard, strict-JSON output, degrade-to-`Hold` |
| **Hook** | fail-closed `PreToolUse` decision core — config-driven tool allowlist, strict order parsing ([ADR-0001](adr/0001-mcp-interaction-model.md) Option 3) |
| **Secrets** | Argon2id + XChaCha20-Poly1305 file vault; `vault:NAME` config refs; values never in config, logs, or API responses |
| **Server** | axum on loopback, bearer auth + 3 RBAC roles, one error envelope, SSE event feed, `/v1/metrics`, rate limit, CORS, serves the dashboard |
| **Dashboard** | React + Vite + TS — token login, PAPER/**LIVE** badge, portfolio, activity, approvals, kill switch, session budget |
| **Backtest** | `sherwood backtest` — total return, drawdown, win rate, profit factor, expectancy |
| **Ops** | `sherwood backup` / `restore`; rolling JSON logs; `deploy/` Prometheus + Grafana; `Dockerfile` + compose; [RUNBOOK.md](RUNBOOK.md) |

## Explicitly not in v0.1 (→ v0.2)

- The Robinhood MCP adapter, OAuth-grant handling, order-status reconciliation
  (S7.4), session reconnect / backoff / supersede (S8).
- The cron scheduler and live price-threshold monitors (S12b).
- A/B decider comparison and a historical quote loader in the backtester (S14b).
- Solana modules (copy-trade, sniper, on-chain signing).
- OS notifications; generated OpenAPI (`docs/API.md` is the maintained contract).

See the [threat-model sign-off](THREAT-MODEL.md#sign-off) for which mitigations
are implemented, partial, or deferred.

## Getting started

```
cp config.example.toml config.toml
export SHERWOOD_VAULT_PASSPHRASE=<yours>
cargo run -p sherwood-cli -- check config.toml
cargo run -p sherwood-cli -- serve config.toml     # API + dashboard on 127.0.0.1:8787
```

Full instructions: [README](../README.md), [DEPLOYMENT.md](DEPLOYMENT.md).

## Operator boundary

Flipping to LIVE mode, wiring a real executor, accepting a venue's agreements,
and placing the first live order are the operator's actions. This project ships
no live adapter and will not perform those steps for you. See
[LIVE_EXECUTION.md](LIVE_EXECUTION.md).
