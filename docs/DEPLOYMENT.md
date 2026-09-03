---
status: stub
last-updated: 2026-09-03
owner-step: S15
---

# Deployment

**Not yet written.** Filled at **S15**.

## Will cover

- Docker image — multi-stage build on `debian:slim` (chosen over distroless for
  debuggability; over Alpine to avoid musl surprises with `rust_decimal` and TLS)
- Docker Compose stack: application + Prometheus + Grafana
- Cross-compilation targets: Windows (development machine), Linux x86-64 (server)
- Installation methods and first-run experience, including local-token generation
- Configuration and secret provisioning per environment
- Backup and restore: `sherwood backup` / `sherwood restore`, and where the backup key lives
- Upgrade path and database migration on startup
- Resource expectations and the single-writer SQLite constraint

Depends on standing question Q2 in [DECISIONS.md](DECISIONS.md) — where this actually runs.
