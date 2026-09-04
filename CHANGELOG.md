# Changelog

All notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Until the first `v0.1.0` release the API and schema may change without notice.

## [Unreleased]

### Added
- **`sherwood-chain` — read-only EVM client (v0.2.1a).** New crate: an
  `EvmClient` trait over JSON-RPC (`HttpClient` on `reqwest`) with the reads
  sherwood needs — `eth_chainId` / `eth_blockNumber` / `eth_call` / `eth_getCode`
  / `eth_getStorageAt` / `eth_getLogs` — plus `keccak256` + selectors, a minimal
  ABI codec, an `Erc20` view (metadata + balances scaled to `Decimal`), and
  `probe::check_transfer_open` — the [ADR-0006](docs/adr/0006-robinhood-chain-venue.md)
  pre-flight in Rust: simulate a `transfer` to a fresh un-onboarded address and
  confirm the token is not allowlist-gated. **No signing, no sends** — there is
  no method that takes a private key. New `sherwood chain-probe [rpc] [token …]`
  subcommand runs it (verified end-to-end against Robinhood Chain mainnet:
  `chainId 4663`, NVDA transfers permissionless). 21 unit tests over a mock
  transport. Deps added: `sha3`, `keccak` (both MIT/Apache, MSRV-safe).
- **`sherwood-chain::univ4` — Uniswap v4 pool price reads (v0.2.1b).** `PoolKey`
  + `PoolId` (`keccak256(abi.encode(...))`), a `StateView` client
  (`getSlot0` / `getLiquidity`), pool discovery from `PoolManager`'s
  `Initialize` logs, liquidity-ranked pool selection, and
  `sqrtPriceX96` → `Decimal` price conversion — computed by shifting both
  operands down 48 bits before squaring, since `sqrtPriceX96` itself can
  exceed `Decimal`'s ~7.9e28 range even though the ratio it represents does
  not. `EvmClient::get_logs` now bisects and retries a block range the node
  refuses (too wide / timed out) instead of failing outright, and
  `HttpClient` backs off and retries a `429`. New `sherwood chain-price [rpc]
  [token] [denom]` subcommand. Verified against a real on-chain pool: an
  NVDA/USDG pool's `sqrtPriceX96` decodes to ≈199 USDG/NVDA, a sane share
  price — recorded as a known-answer test (`pool_id` and the price
  conversion both check against the live values in
  [ROBINHOOD-CHAIN.md](docs/ROBINHOOD-CHAIN.md)). Still no wallet, no
  signing. 7 new unit tests (28 total in `sherwood-chain`).
- **Live paper prices from Robinhood Chain (v0.2.1c).** New `sherwood_chain::feed::ChainFeed`
  — a `sherwood_core::PriceFeed` backed by real Uniswap v4 pool reads. `[chain]` config
  section (`enabled`, `rpc_url`, `symbols`, `denom`, `poll_interval_secs`); when enabled it
  takes over from `feed_path`/the demo feed in `sherwood run`. **Still paper trading** — the
  feed only supplies prices, no wallet, no signing, no order ever reaches the venue. Pool
  discovery happens once per symbol and is cached; every later poll is one cheap `getSlot0`.
  `next_tick` bridges the trait's sync contract to async reads via `block_in_place` +
  `Handle::block_on`; a failing read retries with backoff and, past a cap, sleeps a full poll
  interval rather than spinning or returning `None` (a live feed must never return `None`).
  6 new unit tests, including two on a multi-thread Tokio runtime that exercise the real
  blocking bridge against a mock transport.

  **Bug found and fixed by the first live run:** `find_best_pool` originally discovered *any*
  pool containing the requested token, regardless of what it was paired against, then
  assumed the counter-currency was the requested denominator — so the deepest-by-liquidity
  pool for NVDA turned out to be NVDA paired against an unrelated token, priced as if it were
  NVDA/USDG, giving a result off by roughly 10¹²x. Fixed by filtering pool discovery to the
  exact `(token, denom)` pair (`discover_pools_for_pair`, both `Initialize`-topic orderings);
  `find_best_pool`/`read_price` now take an explicit `denom` address. Covered by a new
  regression test asserting a pool paired against a third token is excluded.
- **`sherwood-signer` — key custody + EIP-1559 signing (v0.2.2).** New crate: `LocalSigner`
  loads a secp256k1 key from the `sherwood-secrets` vault (hex, via `sherwood secrets set` —
  stdin only, never argv), derives its address, and signs an `Eip1559Tx` into raw
  `eth_sendRawTransaction`-ready bytes. **Sign-local, broadcast-explicit**: no RPC client, no
  method that sends anything, and `Debug` never prints the key (`finish_non_exhaustive`) — a
  `sherwood wallet-address <name>` command prints only the derived address. A hand-rolled RLP
  encoder (known-answer tested against the canonical RLP examples) and low-`s` signature
  normalisation (EIP-2) with the recovery id kept consistent, verified by a self-consistent
  sign → recover → address round trip. 21 unit tests. New deps: `k256`, `zeroize`
  (MIT/Apache). [THREAT-MODEL.md](docs/THREAT-MODEL.md#key-custody-v022-sherwood-signer)
  gained a key-custody section.

  **Also fixed while wiring the CLI:** `sherwood secrets set` now strips a leading UTF-8 BOM
  — PowerShell's `Get-Content x | sherwood secrets set` writes one, which silently corrupted
  every secret stored that way (caught by the first `wallet-address` smoke test: a
  clean 64-hex-char key failed to parse as "odd number of digits" until this fix).
- **`sherwood-wallets` — multi-wallet registry (v0.2.3).** New crate: a named `Wallet` wraps a
  `sherwood-signer` key with a symbol allowlist (`allows_symbol`, case-insensitive, empty =
  unrestricted) and a per-wallet spend ceiling (`WalletBudget` — tx count / cumulative
  notional / duration, the same hard-stop-and-latch shape as `sherwood-server`'s per-session
  budget). `WalletRegistry::load` resolves every `key_ref` (a `vault:NAME` reference) against
  the vault and is all-or-nothing — a missing secret, duplicate name, or bad key fails the
  whole load rather than silently dropping a wallet. New `[[wallets]]` config array and
  `sherwood wallets <config.toml>` CLI (prints name / address / allowlist / budget — never a
  key). Same boundary as `sherwood-signer`: no RPC client, no broadcast method; nothing yet
  calls `wallet_for_symbol` to actually pick a wallet for an order — that's `sherwood-dex`.
  14 unit tests.
- **`sherwood-dex` — Uniswap v4 swap construction (v0.2.4).** New crate:
  `ExactInputSingleSwap::execute_calldata` builds a single-hop exact-input `V4_SWAP` through
  the `UniversalRouter` (`060c0f` — `SWAP_EXACT_IN_SINGLE`/`SETTLE_ALL`/`TAKE_ALL`, sourced
  from `Uniswap/universal-router` and `Uniswap/v4-periphery`'s own `Commands.sol` /
  `Actions.sol` / `IV4Router.sol`); `permit2.rs` builds the two prerequisite approvals
  (bounded amounts, no "unlimited" approve); `quote.rs` computes `amount_out_minimum`
  (slippage) and a deadline. New `sherwood dex-simulate <from> <token> <amount_raw> …` — an
  `eth_call` dry run that signs and sends nothing.

  **Verification, honestly reported:** the encoding was diffed byte-for-byte against a real,
  currently-successful `execute` transaction pulled from the live chain — every field matched
  exactly (including `minHopPriceX36`, a struct field added upstream in March 2026, at exactly
  the byte position this crate puts it). That confirms the encoding logic is right. It does
  **not** confirm a swap against a real pool succeeds: `sherwood dex-simulate` against the
  live NVDA/USDG pool still reverts with empty revert data, from a wallet confirmed — at call
  time — to hold ample balance and have both approvals maxed out; smaller amounts and looser
  slippage didn't change the outcome, and re-simulating the real successful transaction (ruling
  out `eth_call` plumbing as the cause) succeeds cleanly. Root cause is open — see
  [`crates/dex/README.md`](crates/dex/README.md). **Do not sign or broadcast anything this
  crate builds until `sherwood dex-simulate` for that exact pool returns success first.**
  19 unit tests (structural correctness only — see above for what they do and don't prove).
  Same boundary as every crate below it: no RPC client, no method that sends anything;
  `eth_sendRawTransaction` does not appear anywhere in this codebase.

### Changed
- **v0.2 re-targeted to Robinhood Chain ([ADR-0006](docs/adr/0006-robinhood-chain-venue.md)).**
  The Robinhood Agentic Trading MCP is US/EEA-gated and unavailable to the operator, so v0.2's
  live venue becomes **Robinhood Chain** — Robinhood's permissionless Ethereum L2 (chain id
  `4663`), traded via Uniswap v4 from a self-custody wallet. Replaces the planned Solana
  modules; their shapes (EVM RPC client, signer isolation, wallet registry, router) carry
  over. v0.1 paper is unchanged. New [`scripts/rhc-probe.mjs`](scripts/README.md) — a
  read-only on-chain probe (signs nothing, sends nothing) that verified Stock Token
  `transfer` is permissionless at the token contract: a fresh un-KYC'd address can receive
  and move tokens, failures are balance-gated only, `paused()` is false, and there is deep
  Uniswap v4 liquidity. The probe becomes the mandatory pre-flight before live mode arms.

## [0.1.0] - 2026-09-04

First feature-complete **paper** release. An agentic-trading control plane: a
deterministic engine, a fail-closed risk gate, a human-in-the-loop approval
gate, per-session spend budgets, an encrypted secret vault, a loopback HTTP API
with a React dashboard, a hash-chained tamper-evident audit log, a backtester,
and Prometheus metrics. **No live-venue path** — the Robinhood MCP adapter,
order reconciliation, and session reconnection are v0.2 (see
[ROADMAP.md](docs/ROADMAP.md) and the [threat-model
sign-off](docs/THREAT-MODEL.md#sign-off)).

### Added
- **Runtime config reload (S2.2)** — `POST /v1/config/reload` (admin + body re-auth)
  re-reads and re-validates `config.toml`, then swaps in the new `[risk]` config, `[hook]`
  tool allowlist, and `approval_mode` under one lock — no restart, no dropped connections or
  in-flight approvals. A broken edit returns the validation error and changes nothing. The
  runtime kill switch is **preserved**: a reload can engage it (if the file says so) but
  never dis-engage one an admin set. `bind`, tokens, CORS, `static_dir`, and the
  session-budget caps still require a restart. The dashboard's Controls card gets a *Reload
  config* button. Internally: `Control` now holds the allowlist and approval mode (both were
  previously immutable in `AppState`), so the reload is a single consistent write.
- **`PreToolUse` hook script (S7.2b)** — [`scripts/pretooluse-hook.mjs`](scripts/README.md),
  the bridge for [ADR-0001](docs/adr/0001-mcp-interaction-model.md) Option 3. A headless
  `claude` / `codex` agent runs it before every tool call; it reads the tool-call event on
  stdin, `POST`s it to `sherwood-server`'s `/v1/hook/pretooluse`, and maps the allow/deny
  answer onto the agent CLI's permission schema. Fails closed on any error, timeout, or
  non-allow. Node ≥ 18, no dependencies. `SHERWOOD_HOOK_CONTEXT` supplies the account/market
  picture the gate checks against (required for buys — otherwise a zero-cash portfolio is
  sent and buys are denied). `--dry-run` prints the request without sending it. The
  script → server → gate → decision chain is tested end-to-end against a local
  `sherwood serve`; driving it from a real agent + Robinhood MCP is the first S7 task once a
  connection exists.
- **`proptest` on the risk gate (S5.7)** — 5 properties over a wide space of configs, orders,
  and portfolio states: `RiskGate::check` never panics and is deterministic; the kill switch
  dominates every other outcome; an accepted order (buy or sell) is always within the
  notional and slippage caps; an accepted buy leaves the position within
  `max_position_fraction`; and a de-risking sell within the notional cap always passes once
  the hard stops are clear. The machine-checked form of the invariants in `SECURITY.md` /
  `AI-SAFETY.md`. (Seeded RNG is N/A — the paper path uses no randomness.)
- **Docker + deployment docs + threat-model sign-off (S15b)** — a multi-stage `Dockerfile`
  (Rust build → dashboard build → `debian:bookworm-slim` runtime, non-root, stripped) and a
  `docker-compose.yml` that runs `serve` with `network_mode: host` (the server is
  loopback-only). `docs/DEPLOYMENT.md` covers bare-binary + systemd, the container, the
  single-writer SQLite constraint, and monitoring. `docs/THREAT-MODEL.md` reviewed against
  the built system and marked `reviewed` — its sign-off splits every mitigation into
  implemented / partial / deferred (v0.2). `.dockerignore` added.
- **`sherwood backup` / `sherwood restore` (S15a)** — `backup <config> <dir>` copies the
  `[general] state_path` SQLite database (with its `-wal` / `-shm` sidecars) and the secret
  vault into a timestamped folder with a `MANIFEST.txt`. `restore <config> <backup-dir>`
  copies them back — it **refuses to overwrite existing live files without `--force`** and
  prints every path it writes. Run both with `serve` / `run` stopped. `docs/RUNBOOK.md` is
  written against what v0.1 actually provides (kill-switch routes, session-budget reset,
  approvals, audit-verify, credential recovery, restore).
- **Observability (S13a)** — `GET /v1/metrics` gains `sherwood_kill_switch`,
  `sherwood_mode_live`, `sherwood_approvals_pending`, `sherwood_session_orders_used`,
  `sherwood_session_notional_used`, and `sherwood_session_budget_breached` gauges — enough
  for real alerting. `$SHERWOOD_LOG_DIR` turns on a daily-rolling JSON log file
  (`tracing-appender`) alongside the console. New [`deploy/`](deploy/README.md): a Prometheus
  scrape config, alert rules (server down, kill switch, budget breached, approvals backlog,
  LIVE mode, high 5xx), and an importable Grafana dashboard. `docs/OBSERVABILITY.md` written.
- **`sherwood backtest` (S14a)** — replays the `[general] feed_path` CSV through the
  configured decider and the risk gate (the same `run_loop` as `sherwood run`, same paper
  executor) and prints a performance summary: total return, max drawdown (peak-to-trough of
  the per-tick equity curve), fills, closed trades + win rate, gross profit / loss, profit
  factor, expectancy per trade. Closed-trade P&L uses average-cost basis; a partial sell
  closes a proportional slice. Deterministic, nothing persisted, `order_cooldown_secs` forced
  to `0`. See [docs/BACKTEST.md](docs/BACKTEST.md) for what it does and does **not** tell you.
- **Per-session spend budgets (S12a)** — three `[server]` hard stops independent of the risk
  config: `max_session_orders`, `max_session_notional`, `max_session_duration_secs` (any `0`
  = unlimited). Once a cap latches, `POST /v1/hook/pretooluse` denies every further
  place-order (`"session budget: …"`) until an admin calls `POST /v1/session/reset` (with
  body re-auth). Reads and cancels never touch the budget. `GET /v1/session` shows
  `{ orders_used/cap, notional_used/cap, elapsed_secs/cap, breached }`; the dashboard shows a
  budget line with a Reset button. The cron scheduler and price-threshold monitors (the rest
  of S12) are deferred — they drive the run loop, which `serve` does not host.
- **Approval gate (S11)** — a human-in-the-loop step between the risk gate and the venue
  ([ADR-0005](docs/adr/0005-approval-gate.md)). `[server] approval_mode`: `auto` (default,
  transparent — unchanged) or `manual`. In `manual` mode a risk-passing **place-order** call
  creates a `pending` approval and `POST /v1/hook/pretooluse` holds its response until the
  operator approves or denies it, or `approval_timeout_secs` (default 60) elapses and it
  auto-denies. Reads and cancels are never held. New routes: `GET /v1/approvals`
  (`{ mode, pending, approvals[] }`, viewer) and `POST /v1/approvals/{id}`
  (`{ decision: "approve" | "deny", reason? }`, operator). A 5s sweeper expires stale
  pendings even with no hook waiting. In-memory, capped history; a restart denies anything
  pending. Dashboard gains an `Approvals` card with per-order Approve / Deny.
- **Live event feed (S9d)** — `GET /v1/events` is a Server-Sent Events stream (viewer role,
  same bearer auth as every route). Each frame carries a JSON array of audit-chain rows
  appended since the last one; an empty array plus a keep-alive comment when nothing changed.
  The dashboard consumes it with a `fetch`-based SSE reader (so the token stays in a header,
  which `EventSource` can't do) and shows activity from the stream, dropping the `/v1/activity`
  poll to a slow fill-count/fallback. SSE rather than a WebSocket — one-directional, native
  browser reconnect, no upgrade handshake to secure. `sherwood serve` stays a control plane
  and does not run the trading loop (see `docs/DECISIONS.md`).
- **Server serves the dashboard (S10.1)** — `[server] static_dir` (e.g. `frontend/dist`)
  makes `sherwood-server` serve the built dashboard at `/` with SPA fallback to
  `index.html`, so the whole thing runs from one loopback origin. Static responses carry a
  strict `Content-Security-Policy` (matching the build-time one), `X-Frame-Options: DENY`,
  `X-Content-Type-Options: nosniff`, and `Referrer-Policy: no-referrer`. `/v1/*` still takes
  precedence; config validation checks the directory has an `index.html`.
- **Dashboard (S10)** — `frontend/`: a React + Vite + TypeScript control panel for
  `sherwood-server`. Token login held in `sessionStorage` (this tab only, never on disk),
  dropped on any `401`. Status bar with the always-visible PAPER / **LIVE** badge (live is
  red and pulses) and kill-switch indicator; portfolio card (cash, realized P&L, positions);
  activity list with a chain-integrity badge from `GET /v1/audit/verify`; admin controls for
  the kill switch and the PAPER/LIVE toggle, each re-prompting for the admin token. No UI
  framework — one stylesheet in the shadcn idiom. Strict CSP injected at build time. New CI
  job runs `npm ci` · lint · typecheck · build. See
  [docs/FRONTEND-ARCH.md](docs/FRONTEND-ARCH.md).
- **Read-only state views on the server (S11a)** — `sherwood serve` opens the same
  `[general] state_path` database `sherwood run` writes, and exposes it: `GET /v1/portfolio`
  (cash, realized P&L, open positions), `GET /v1/activity?limit=N` (recent audit-chain
  events + fill count), `GET /v1/audit/verify` (recompute the hash chain — `{ ok, entries }`
  or `{ ok: false, broken_at }`). All viewer-role; `404` through the envelope when no
  `state_path` is configured or no snapshot exists yet. Gives the dashboard (S10) real data
  before the full approval gate.
- **Metrics, rate limit, CORS (S9c)** — `GET /v1/metrics` returns Prometheus text
  (hand-rolled counters — request totals, status-class breakdown, uptime, plus `kill_switch`
  and `mode_live` gauges; no `prometheus` crate). A global fixed-window rate limit
  (`[server] rate_limit_per_min`, default 120, `0` disables) returns `429` through the error
  envelope. `[server] cors_origins` allows the dashboard's browser origin; empty = same-origin
  only. The WebSocket event feed and generated OpenAPI are deferred to S9d — both want the
  run loop folded into the server first.
- **RBAC, PAPER/LIVE toggle, kill switch (S9b)** — `sherwood-server` gains three roles
  (`viewer` < `operator` < `admin`), assigned by which configured token authenticates
  (`token_ref` = admin; optional `operator_token_ref` / `viewer_token_ref`). `require_auth`
  middleware stamps the role; each route declares its minimum via a `Caller` extractor. New
  routes: `GET /v1/control` (viewer), `POST /v1/mode` and `POST /v1/kill` — both admin **and**
  the admin token again in the request body (`reauth`). LIVE mode is refused unless
  `[server] allow_live = true`. The kill switch flips `RiskConfig.kill_switch` under an
  `RwLock<Control>`, so an engaged switch immediately makes `POST /v1/hook/pretooluse` deny
  every order. `/v1/health` now reports `kill_switch`.
- **`sherwood-server` skeleton + `sherwood serve` (S9a)** — the local control-plane HTTP API.
  axum bound to loopback (a non-loopback bind is refused — TLS is a later concern); one
  `{ code, message, correlation_id }` error envelope on every failure; bearer-token auth
  with a constant-time compare (`subtle`), the token generated into the `sherwood-secrets`
  vault on first run and never logged. Routes: `GET /v1/health` (open) and
  `POST /v1/hook/pretooluse` (bearer) which runs S7's `HookGate` against a caller-supplied
  portfolio + market context — a *denied* tool call is a `200` with `{"decision":"deny",…}`,
  only a malformed request is a `4xx`. New `[server]` (bind, `token_ref`) and `[hook]`
  (`read_tools` / `place_tools` / `cancel_tools`) config sections. 13 new tests.
- **`PreToolUse` hook decision core (S7.1–S7.2)** — `sherwood-execution::hook`, the
  fail-closed choke point for [ADR-0001](docs/adr/0001-mcp-interaction-model.md) Option 3.
  `HookGate::evaluate` takes an intercepted agent `ToolCall` and returns `HookOutcome::Allow`
  or `Deny { reason }`. A tool that is not on the `ToolAllowlist` is denied; an order tool
  whose arguments do not parse is denied (never passed through); a parsed order is denied
  unless it passes `RiskGate::check` unchanged; read-only and cancel tools pass without a
  risk check, and cancels pass even when a hard stop is engaged. The allowlist is
  config-driven — no Robinhood tool names are baked in (ADR-0001 open item). `order_parse`
  maps agent tool arguments to a `core::Order` strictly (`symbol` + `side` + `quantity`, or
  `notional` plus a price; `$`/`,` tolerated; floats read via their exact text). The hook's
  HTTP surface and agent-process supervision land with S9. 25 new tests.
- **AI decider (S4)** — `[general] decider = "ai"` drives the paper loop with a language
  model instead of the threshold rules. `AiProvider` trait + `OpenAiCompatProvider`
  (`reqwest` + rustls, whole-request timeout) behind the `decision/openai` feature, working
  against any OpenAI-compatible `/chat/completions` endpoint (NVIDIA NIM, Groq, a local
  server). The `decision` crate owns the prompt: untrusted market data goes in a
  `<market_data>` block, the symbol field is scanned for injection markers, output is
  strict JSON (`deny_unknown_fields`) with a code-fence strip, one retry, then
  degrade-to-`Hold`. Per-run call budget (`ai.max_calls_per_run`) and an optional symbol
  `universe`. The API key is a `vault:` reference resolved at load — a literal key in the
  config is rejected. Every proposal still passes `RiskGate`; the runner stays paper-only.
  15 new tests. See [docs/AI-SAFETY.md](docs/AI-SAFETY.md).
- **`sherwood-secrets` (S6)** — a `FileVault`: Argon2id-derived key, XChaCha20-Poly1305 over a
  JSON `name → value` map, passphrase from `$SHERWOOD_VAULT_PASSPHRASE` (`0600` on Unix).
  `SecretString` zeroes on drop and prints `[redacted]`; `resolve_ref` turns `vault:NAME`
  config references into values. New `sherwood secrets set|get|list|rm` — `set` reads from
  stdin, not argv. 7 tests incl. wrong-passphrase and tamper detection.
- **Multi-asset paper loop + CSV feed (S5.1–5.2)** — `PriceFeed` trait + `Tick` in `core`;
  `CsvFeed` (replays `timestamp,symbol,price` rows) and `SliceFeed` in the CLI. The run loop
  is now driven by a feed, one symbol per tick, with equity and unrealized P&L marked
  against the latest price per held symbol. `[general] feed_path` selects a CSV; `feeds/demo.csv`
  is a runnable sample. The built-in demo feed trades two symbols. Closes CURRENT-STATE
  defects 2 and 3.
- **`RiskGate` extension (S5)** — hard stops (kill switch, realized daily loss) vs entry
  limits. New entry limits, all buy-only so a de-risking sell always passes: an
  **unrealized-loss breaker**, **max concurrent open symbols**, and a **per-symbol buy
  cooldown**. The gate now takes a `GateContext`; `now` is injected via a new
  `sherwood_core::Clock` (`SystemClock` / `FixedClock`). `Portfolio` gains
  `unrealized_pnl` and `open_position_count`. Closes CURRENT-STATE defects 8 and 12.
- **`sherwood-events` (S3)** — internal bus (`tokio::sync::broadcast`, bounded 1000)
  behind a `Subscriber` trait. Four event variants, each with a real emitter and
  consumer; every message a versioned `Envelope` (ADR-0004). `TracingSubscriber`
  (always on) and `StoreSubscriber` (persists fills + audit rows). A slow or failing
  subscriber is logged, never fatal. The run loop now publishes events instead of
  calling the store directly.
- **`sherwood-store` (S1)** — SQLite persistence via `sqlx` behind a `Store` trait:
  portfolio snapshots, fill history, and a hash-chained tamper-evident `audit_log`
  with `verify_audit_chain`. Compile-time-checked queries against a committed
  `.sqlx/` offline cache; `SQLX_OFFLINE` in CI so no database is needed.
- `sherwood run` with `[general] state_path` set now persists: it resumes from the
  last portfolio snapshot, records every fill and gate rejection to the audit
  chain, and snapshots on exit (clean interrupt included).
- `Portfolio` is `serde`-serialisable, with a JSON round-trip test.
- Pre-S0 repository hygiene: committed `Cargo.lock`, `deny.toml`, Renovate config,
  workspace lint manifest, `CODEOWNERS`, PR and issue templates, `CLAUDE.md`.
- Repository made public; branch protection enabled on `main`.
- CI expanded: MSRV 1.80 build, `cargo-deny` (licences + RustSec advisories + bans +
  sources), CycloneDX SBOM, `gitleaks`, a coverage report, and a doc-link check.

### Changed
- `PaperExecutor` recovers from a poisoned mutex instead of unwrapping.
- `runner` guards against an empty price series instead of indexing.
- Config validation and graceful shutdown (`docs/CURRENT-STATE.md` defects 9 and 10).

## [0.0.1] - 2026-09-03

The scaffold and the S0 planning documentation. Tagged as the hygiene milestone;
the workspace version remains `0.1.0` (the in-progress target).

### Added
- Rust workspace: `core`, `execution`, `decision`, `copytrade`, `sniper`, `cli`.
- `RiskGate` with eight rejection reasons; `Portfolio` ledger; deterministic
  `PaperExecutor`; `RuleDecider` and an `AiDecider` closure wrapper.
- The S0 documentation set under `docs/`, including three accepted ADRs.

[Unreleased]: https://github.com/dunksmaster/sherwood-agent/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/dunksmaster/sherwood-agent/compare/v0.0.1...v0.1.0
[0.0.1]: https://github.com/dunksmaster/sherwood-agent/releases/tag/v0.0.1
