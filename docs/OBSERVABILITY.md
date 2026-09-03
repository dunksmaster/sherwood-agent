---
status: stub
last-updated: 2026-09-03
owner-step: S13
---

# Observability

**Not yet written.** Filled at **S13**.

## Decided already

Prometheus and Grafana are **deployed alongside, never vendored**. The application exposes a
`/metrics` endpoint and ships Grafana dashboard definitions as JSON.

## Will cover

- Metric catalogue: decisions by outcome, gate rejections by reason, order latency, fill
  rate, session state, AI cost and token usage, event-bus depth and drops
- Log destination and rotation — local JSON, 7-day default retention
- Grafana dashboards shipped as provisioned JSON
- **Alert thresholds**, at minimum:
  - daily loss reaching 80% of the configured limit
  - venue session down beyond 60 seconds
  - kill switch engaged
  - repeated gate denials within a window
  - AI budget at 80% consumed
  - event-bus backpressure warnings
- Notification channels: OS notification for critical, webhook for the rest
- The audit feed UI, including chain verification
