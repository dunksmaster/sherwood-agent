---
status: partial
last-updated: 2026-09-04
owner-step: S15
---

# Runbook

Operational procedures for a running `sherwood serve` / `sherwood run`. Scoped to
what exists in v0.1 (paper). Items that need the live venue are marked
**(v0.2 / S7.4)**.

## Stop trading now

The kill switch rejects **every** order (buy and sell) at the risk gate until an
admin releases it. Three ways in, one effect:

1. **Dashboard** → Controls card → *Engage kill switch* (needs the admin token
   in the re-auth field).
2. **API**: `POST /v1/kill` with `{ "engage": true, "reauth": "<admin token>" }`
   (admin bearer + body re-auth).
3. **Config + restart**: set `[risk] kill_switch = true` and restart. Use this
   if the server is unreachable.

There is no `sherwood kill` subcommand and no sentinel file yet — both are
planned. `Ctrl-C` on `serve` / `run` stops the process cleanly (finishes the
current tick, snapshots if `state_path` is set); it does not "engage the kill
switch", it ends the run.

**In-flight orders:** in paper mode there are none — `PaperExecutor` fills
synchronously. Under Option 3 with a live agent, an order already handed to the
Robinhood MCP is the venue's; the hook only blocks *new* calls **(v0.2 / S7.4)**.

**Release the kill switch:** same three routes, `engage: false`. First verify:
`GET /v1/health` shows the cause is gone, `GET /v1/session` is not `breached`,
`GET /v1/audit/verify` is `ok`.

## Diagnose

| Symptom | Check | Action |
|---|---|---|
| Hook denies every order | `GET /v1/session` — `breached: true`? | `POST /v1/session/reset` (admin + re-auth). Confirm the caps are what you want in `[server]`. |
| Hook denies every order, session not breached | `GET /v1/health` — `kill_switch: true`? | Release the kill switch (above). |
| Orders sit in `manual` approval | `GET /v1/approvals` | Approve / deny each. To stop holding new orders: set `approval_mode = "auto"` in `config.toml` and `POST /v1/config/reload` (admin + re-auth) — no restart. Pending approvals still auto-deny after `approval_timeout_secs`. |
| Risk caps or the tool allowlist are wrong | — | Edit `config.toml`, then `POST /v1/config/reload`. A bad edit is rejected with the validation error and nothing changes. `bind` / tokens / CORS / `static_dir` / session-budget caps still need a restart. |
| `GET /v1/audit/verify` → `ok: false` | note `broken_at` | The state DB was altered outside the app, or is corrupt. Stop the server. Restore from backup (below). Do **not** keep trading against a broken chain. |
| Dashboard can't reach the API | `curl -s localhost:8787/v1/health` | Server down, or bound elsewhere — check `[server] bind` and the process. `401` means the token is wrong: `sherwood secrets get api_token`. |
| `429` from the API | rate limit | One client is hammering it; raise `[server] rate_limit_per_min` or `0` to disable. |
| AI decider always `Hold` | server / run logs | Provider error, timeout, injection flag, or the per-run call budget (`ai.max_calls_per_run`) is exhausted. `Hold` is the safe fallback; check `ai.base_url` / the vault key. |
| Vault won't open | — | `$SHERWOOD_VAULT_PASSPHRASE` is wrong or unset, or the file was tampered with (AEAD fails closed). Restore the vault from backup. |

## Recover

### Restore state from a backup

Stop `serve` / `run` first (SQLite WAL sidecars must be quiescent).

```
sherwood backup  config.toml  /path/to/backups          # take one
sherwood restore config.toml  /path/to/backups/sherwood-backup-<stamp> --force
```

`restore` refuses to overwrite live files without `--force` and prints every
file it writes. After restoring, run `sherwood serve` and check
`GET /v1/audit/verify`. Reconstructing state *after* the last snapshot from the
venue's order ledger is **(v0.2 / S7.4)**.

### Lost or rotated credentials

- **API token:** `sherwood secrets rm api_token` then restart `serve` — a fresh
  one is generated and printed once. Update the dashboard / clients.
- **Provider (NVIDIA/…) key:** `sherwood secrets set nvidia` (reads new value
  from stdin). No restart needed for the next run.
- **Vault passphrase:** cannot be recovered. If lost, the vault is gone —
  restore it from a backup taken while you still had the passphrase, or
  re-create every secret.

### Roll back a bad release

`git checkout <previous tag>` and rebuild, or `docker run` the previous image.
State is forward-compatible within v0.1 (no destructive migrations); a rollback
does not need a state restore unless the bad release wrote bad data — in which
case restore the last good backup.

### Before restarting after an incident

Capture: the log directory (`$SHERWOOD_LOG_DIR`), a fresh `sherwood backup`,
`GET /v1/audit/verify` output, and the last ~200 audit rows
(`GET /v1/activity?limit=200`).

## Routine

- **Backups:** `sherwood backup` before every config change and on a schedule
  you keep off-box. Verify one occasionally by restoring into a scratch dir.
- **Audit chain:** `GET /v1/audit/verify` daily (the dashboard badge shows it
  live).
- **Dependencies:** `cargo deny check` and the CI advisory scan on every push;
  review Renovate PRs weekly.
