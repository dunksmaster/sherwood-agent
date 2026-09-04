---
status: partial
last-updated: 2026-09-04
owner-step: S15
---

# Deployment

`sherwood` is a **single-operator, loopback-only** tool (see
[SECURITY.md](SECURITY.md)). There is no "expose it to the network" recipe: the
server refuses any non-loopback bind because it has no TLS and one shared
bearer token. Deploy it next to the operator, or behind their own VPN / SSH
tunnel.

## Bare binary (recommended)

```
cargo build --release -p sherwood-cli
./target/release/sherwood serve config.toml
```

- Set `$SHERWOOD_VAULT_PASSPHRASE` (required) and, optionally, `$SHERWOOD_LOG_DIR`
  for a rolling JSON log and `$SHERWOOD_VAULT_PATH` to place the vault.
- First `serve` mints the API token into the vault and prints it once —
  `sherwood secrets get api_token` to retrieve it later.
- Build the dashboard once (`cd frontend && npm ci && npm run build`) and point
  `[server] static_dir = "frontend/dist"` at it, or run `npm run dev` separately.

### systemd unit (Linux)

Keep the passphrase out of the unit file — put it in a `0600`
`EnvironmentFile` (`SHERWOOD_VAULT_PASSPHRASE=<your passphrase>`, one line):

```ini
[Service]
EnvironmentFile=/etc/sherwood/env
Environment=SHERWOOD_LOG_DIR=/var/lib/sherwood/logs
WorkingDirectory=/var/lib/sherwood
ExecStart=/usr/local/bin/sherwood serve /etc/sherwood/config.toml
Restart=on-failure
DynamicUser=yes
StateDirectory=sherwood
```

## Docker

The [`Dockerfile`](../Dockerfile) is a three-stage build (Rust → dashboard →
`debian:bookworm-slim` runtime, non-root, `strip`ped binary). Chosen over
distroless for debuggability and over Alpine to avoid musl surprises with TLS.

```
docker build -t sherwood-agent .
docker run --rm sherwood-agent --help
docker run --rm -v "$PWD:/work" -w /work sherwood-agent backtest config.toml
```

For `serve`, [`docker-compose.yml`](../docker-compose.yml) uses **`network_mode:
host`** (Linux) so the loopback bind still works and nothing reaches the wider
network. Export the vault passphrase into your shell (or a gitignored `.env`),
then `docker compose up -d`.

`sherwood-data` (state DB + vault) and `sherwood-logs` are named volumes. On
macOS / Windows, `host` networking is unavailable — run the binary directly.

The image build is not yet CI-gated.

## Monitoring

Prometheus and Grafana run on the host (or the operator's existing stack), not
in this compose file — see [`deploy/`](../deploy/README.md).
[`deploy/prometheus.yml`](../deploy/prometheus.yml) already targets
`127.0.0.1:8787`.

## Backup / restore

Stop `serve` / `run` first (SQLite WAL sidecars must be quiescent):

```
sherwood backup  config.toml  /path/off-box
sherwood restore config.toml  /path/off-box/sherwood-backup-<stamp> [--force]
```

`restore` won't overwrite live files without `--force`. Keep backups on an
encrypted volume — the vault is AEAD-encrypted, the state DB is not (see
[SECURITY.md](SECURITY.md#data-at-rest)).

## Constraints

- **SQLite is single-writer.** One `serve` **or** one `run` per `state_path` at a
  time — not both, not two of either.
- **Migrations run on open**, forward-only within v0.1; a downgrade needs a
  restore.
- Resource footprint is small (tens of MB RSS idle); the AI decider's provider
  calls are the only outbound traffic.

Standing question Q2 (where this runs) in [DECISIONS.md](DECISIONS.md) resolves
to: local first, Docker for portability.
