---
status: accepted
last-updated: 2026-09-03
owner-step: S0
---

# Security

This software places orders against a funded brokerage account. Security failures here cost
money directly. The threat analysis lives in [THREAT-MODEL.md](THREAT-MODEL.md); model-
specific concerns in [AI-SAFETY.md](AI-SAFETY.md). This document is the architecture and
the policy.

## Reporting a vulnerability

Do not open a public issue. Use GitHub's private vulnerability reporting on this repository
("Report a vulnerability" under the Security tab). Include impact, reproduction, and affected
versions. Please allow a reasonable window before public disclosure.

Areas of particular interest: anything that lets an order reach a venue without passing
`RiskGate`; credential handling; the local API and hook surface; the release pipeline.

Out of scope: trading losses from strategy decisions or market risk.

## Core invariants

These are the properties the design exists to preserve. A change that breaks one is a
security defect regardless of intent.

1. **No order reaches a venue without passing `RiskGate::check`.**
2. **Every control fails closed.** Unreachable, errored, or ambiguous means "do not trade".
3. **Credentials never enter the codebase, the database, logs, or API responses.**
4. **The audit log is append-only and tamper-evident.**
5. **Live mode is never the default and never implicit.**

## Credentials

v0.1 uses Robinhood OAuth. There are no private keys — Robinhood custodies the assets. The
credential surface is therefore: the OAuth grant, the NVIDIA (or other provider) API key, and
the local API token.

| Rule | Detail | State |
|---|---|---|
| Storage | `sherwood-secrets` `FileVault` — Argon2id-derived key, XChaCha20-Poly1305 over a JSON map. Passphrase from `$SHERWOOD_VAULT_PASSPHRASE`, never written. `0600` on Unix. An OS-keyring backend is a later option. | **done** |
| Configuration | Config holds *references* — `api_key = "vault:nvidia"` — resolved by `resolve_ref` at load. Values never appear in a config file. | **done** (resolver); consumers land at S4 |
| In memory | `SecretString` zeroes on drop; `Debug` prints `SecretString([redacted])`. | **done** |
| CLI | `sherwood secrets set` reads the value from **stdin**, never argv (argv is visible in `ps`). `get` prints plaintext and says so. | **done** |
| Logs | A `tracing` layer scrubs known secret shapes; secrets are never formatted into errors. | pending (S13) |
| API | Write-only — the dashboard sets a secret, no endpoint returns one; UI shows `••••••`. | pending (S9) |
| Rotation | `secrets set` overwrites in place; live reload is S2.2. | partial |
| Revocation | Losing the OAuth grant is a `Fatal` error: halt, emit `SessionStateChanged`, require operator re-consent. Never silently retry a 401. |

Under [ADR-0001](adr/0001-mcp-interaction-model.md) Option 3, the OAuth grant lives inside the
CLI agent's own credential store and never reaches our code at all. That is a principal
reason to prefer it.

Key custody tiers (paper → hot → hardware → HSM) are a **v0.2** concern and arrive with
Solana. They are deliberately absent from v0.1 because v0.1 holds no keys.

## Spend controls

Enforced in `sherwood-core`'s `RiskGate`, independent of any strategy or model, on every
order. Two classes:

**Hard stops — reject every order, buy or sell:**

- Kill switch
- Realized daily-loss circuit breaker

**Entry limits — gate new exposure; a sell that de-risks always passes:**

- Maximum notional per order · maximum position as a fraction of equity
- **Unrealized-loss breaker** (mark-to-market across open positions)
- **Maximum concurrent open symbols**
- **Per-symbol cooldown between buys** (`now` from an injected `Clock`, never the wall clock)

Still to come: per-day and per-symbol-per-day notional caps, per-run budgets (S5/S12).

Defaults are deliberately small. Raising them is a config change that will be recorded in the
audit log once config lives in the store (S2).

## Kill switch

Reachable three ways, all converging on the same halt:

1. A button in the dashboard (admin role, confirmation dialog).
2. `sherwood kill` on the command line.
3. A sentinel file on disk — checked on every gate evaluation, so it works even if the
   server is wedged.

Engaging it rejects every order immediately and emits `KillSwitchToggled`. Disengaging
requires the admin role and is audited. Recovery procedure lives in
[RUNBOOK.md](RUNBOOK.md).

## Local API

- Binds `127.0.0.1` by default. Any other bind is **refused** (a non-loopback bind needs TLS,
  which is a later concern). *Done (S9a).*
- Bearer token, generated on first `serve` into the `sherwood-secrets` vault (an OS-keyring
  backend is the deferred S6.3 option), compared in constant time (`subtle`). *Done (S9a).*
- RBAC — `viewer` (read), `operator` (the `PreToolUse` hook), `admin` (mode toggle, kill
  switch). The role is whichever configured token authenticated; each route declares its
  minimum. Wired even though v0.1 usually runs admin-only, so adding tokens later is not a
  redesign. *Done (S9b).*
- The live toggle and the kill switch require the admin role **and** the admin token again in
  the request body. LIVE is additionally gated by `[server] allow_live`. *Done (S9b).*
- CORS headers are emitted only for origins in `[server] cors_origins` (empty = same-origin
  only). Strict CSP on the frontend, no external script/style origins: *pending (S10).*
- Global fixed-window rate limit (`[server] rate_limit_per_min`), `429` through the error
  envelope. *Done (S9c).* Per-token limits and the WebSocket upgrade path: *pending (S9d).*

## The approval hook

Under ADR-0001 Option 3 the `PreToolUse` hook is the only thing between the agent and the
venue. It is treated as security-critical:

- **Fails closed.** No response, a timeout, or a malformed response means deny. The decision
  core (`sherwood-execution::hook`, landed at S7) also denies on an unparseable payload, an
  order tool whose arguments do not parse, or any `RiskGate` rejection.
- **Tool allowlist.** Only explicitly named venue tools are gated *and* permitted; an
  unrecognised tool name is denied, not passed through. The allowlist is configuration — no
  tool names are compiled in — and each entry is classified read-only / place-order /
  cancel-order. Only place-order calls are parsed and risk-checked; a cancel is allowed even
  under a hard stop, since it can only reduce exposure.
- Authenticated with the local token; an unauthenticated hook call is denied and alerted.
- Its own timeout must exceed the approval timeout, and the agent's hook timeout must exceed
  both, or a slow human approval is silently converted into a denial.

## Data at rest

- SQLite encrypted with SQLCipher (feature-gated) or relying on full-disk encryption; the
  choice is recorded per deployment.
- Backups are encrypted; the backup key is stored separately from the backups and its
  recovery procedure is documented in the runbook.
- Order history and account identifiers are treated as sensitive personal data.
- Retention: market snapshots default to 90 days; audit and fills are retained indefinitely.

## Supply chain

- `cargo-deny` — licences, advisories, bans, source allowlist.
- `cargo-audit` and `osv-scanner` (frontend) in CI.
- Lockfiles committed; Renovate proposes updates.
- SBOM (CycloneDX) generated per release and attached to the release.
- `gitleaks` in CI and as a pre-commit hook.
- Signed tags; release provenance via GitHub OIDC.

## Disclosure posture

The repository is **public** — a deliberate choice to develop this as an open-source project
(standing question Q7, resolved 2026-09-03; see [DECISIONS.md](DECISIONS.md)).

The security consequence is accepted and compensated for:

- **The design is public, so the controls must not depend on it being secret.** Every
  invariant in this document holds against a reader who knows exactly how the system works.
  Nothing here is security-through-obscurity.
- **No secret has ever been committed.** History is scanned by `gitleaks` and a third-party
  scanner on every push; both are green. `config.toml`, `.env`, `*.key`, and keypair files
  are gitignored and were never tracked.
- **The operator's identity and the fact they run this is inferable.** That is a real
  residual risk of a public trading-bot repo. It is accepted. Personal operational details
  (wallet addresses, account numbers, RPC endpoints, API keys) live only in the local,
  gitignored config and never in the repository.
- **`main` is branch-protected** — no direct pushes, PR + green CI required, linear history.
  A drive-by PR cannot weaken a control without the checks catching it.

If a future change would only be safe while the design is private, that change does not
belong in this project.

## Non-goals for v0.1

Multi-user identity, SSO, hardware custody, HSM or MPC signing, regulatory compliance
tooling, and cloud multi-tenancy are explicitly out of scope. RBAC is wired but exercised by
a single operator.
