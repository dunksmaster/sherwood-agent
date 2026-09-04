---
status: partial
last-updated: 2026-09-04
owner-step: S13
---

# Observability

Prometheus and Grafana run **alongside, never vendored** (see
[DECISIONS.md](DECISIONS.md)). `sherwood-server` exposes `/v1/metrics`; the
artifacts to consume it are in [`deploy/`](../deploy/README.md).

## Metric catalogue (`GET /v1/metrics`)

Unauthenticated — the server binds loopback only. Prometheus text format.

| Metric | Type | Meaning |
|---|---|---|
| `sherwood_requests_total` | counter | HTTP requests handled |
| `sherwood_responses_total{class}` | counter | responses by status class (`1xx`…`5xx`, `other`) |
| `sherwood_uptime_seconds` | gauge | seconds since server start |
| `sherwood_kill_switch` | gauge | `1` when the kill switch is engaged |
| `sherwood_mode_live` | gauge | `1` in LIVE mode |
| `sherwood_approvals_pending` | gauge | orders awaiting operator approval |
| `sherwood_session_orders_used` | counter | place-orders allowed this session |
| `sherwood_session_notional_used` | gauge | cumulative allowed notional this session |
| `sherwood_session_budget_breached` | gauge | `1` when a per-session budget cap has latched |

Still to add (need the run loop hosted in `serve`, or the live venue): decisions
by outcome, gate rejections by reason, order latency, fill rate, AI cost / token
usage, event-bus depth.

## Logs

Console always. Set `SHERWOOD_LOG_DIR` to also write a **daily-rolling JSON**
file (`sherwood.log.YYYY-MM-DD` in that directory). Retention is the operator's —
prune old files or point `logrotate` at the directory; 7 days is the documented
default. `RUST_LOG` controls levels (default `info`).

## Alerts

[`deploy/prometheus-alerts.yml`](../deploy/prometheus-alerts.yml):

| Alert | Fires when | Severity |
|---|---|---|
| `SherwoodDown` | not scraped for 1m | critical |
| `KillSwitchEngaged` | `sherwood_kill_switch == 1` | critical |
| `SessionBudgetBreached` | a budget cap latched | warning |
| `ApprovalsBacklog` | an order pending approval for 5m | warning |
| `LiveMode` | server in LIVE mode | info |
| `HighServerErrorRate` | >5% 5xx over 5m | warning |

The daily-loss-at-80% and venue-session-down alerts named in the original stub
arrive with order reconciliation (S7.4) and the run loop's own metrics.

## Dashboard

[`deploy/grafana-dashboard.json`](../deploy/grafana-dashboard.json) — import and
select a Prometheus datasource. Stat tiles for kill switch / mode / pending
approvals / budget-breached; timeseries for request rate by class and session
notional.

## Audit-feed UI

The dashboard's Activity card streams audit-chain rows over SSE
(`GET /v1/events`) and shows a live **chain-integrity badge** driven by
`GET /v1/audit/verify` — green with the entry count, or red with the first
broken sequence number.

## Not yet

OS notifications for critical events, and a webhook channel for the rest, are
deferred: `serve` runs headless (often as a service), where desktop toasts are
unreliable, and a webhook needs a configured, trusted endpoint. Prometheus
Alertmanager already covers notification routing for the metrics above.
