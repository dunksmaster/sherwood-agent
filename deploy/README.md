# deploy/

Observability artifacts for `sherwood-server`. Prometheus and Grafana run
**alongside**, not vendored (see [DECISIONS.md](../docs/DECISIONS.md)).

| File | What |
|---|---|
| `prometheus.yml` | Minimal scrape config — polls `http://127.0.0.1:8787/v1/metrics` every 15s and loads the alert rules. |
| `prometheus-alerts.yml` | Alert rules: server down, kill switch engaged, session budget breached, approvals backlog, LIVE mode, high 5xx rate. |
| `grafana-dashboard.json` | An importable dashboard — kill switch / mode / pending approvals / budget-breached stat tiles, request-rate and session-notional timeseries. Pick your Prometheus datasource on import. |

```
prometheus --config.file=deploy/prometheus.yml
```

`/v1/metrics` is unauthenticated because the server binds loopback only. Full
metric catalogue and alert-threshold rationale: [OBSERVABILITY.md](../docs/OBSERVABILITY.md).
