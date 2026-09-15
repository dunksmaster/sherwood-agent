# sherwood-agent v0.2.14

Robinhood Chain integration, feature-complete. **Still paper trading only** —
there is no broadcast capability anywhere in this codebase, and none is
planned; see [ADR-0007](adr/0007-no-broadcast-capability.md).

## What it is

v0.1 shipped a paper-trading control plane with no live venue at all. v0.2
re-targeted the live venue to Robinhood Chain — a permissionless EVM L2 — and
built every piece of the read/simulate/reconcile path an operator needs to
trade there manually, while keeping the bundled runner paper-only by
construction:

1. **Read the chain.** A hand-rolled JSON-RPC client (`sherwood-chain`),
   Uniswap v4 pool discovery and price reads, and a live price feed wired into
   the same `PriceFeed` trait the paper loop already used.
2. **Hold keys, never send.** A local signer (`sherwood-signer`) and a
   multi-wallet registry with per-wallet spend ceilings (`sherwood-wallets`) —
   both can produce a signature; neither can submit a transaction. No RPC
   method for broadcasting a signed transaction exists in this codebase.
3. **Build and simulate a swap.** `sherwood-dex` constructs real Uniswap v4
   swap calldata; `sherwood dex-simulate` / `POST /v1/dex/simulate`
   `eth_call`-simulates it against the live chain before the operator ever
   signs anything.
4. **Route the decision.** `sherwood-router` picks AMM vs. RFQ by notional —
   pure logic, no RPC — and the runner logs which wallet/venue a paper fill
   would have used live, once `[[wallets]]` are configured.
5. **Close the loop after a manual broadcast.** The operator signs and sends
   with their own tooling; `sherwood-reconcile` / `POST /v1/reconcile` reads
   the resulting receipt and records a fill exactly like a paper one.
6. **Operate all of it from the dashboard**, not just the CLI: a config
   editor (`GET`/`POST /v1/config`), a cash-over-time chart, and cards for
   the router / dex-simulate / reconcile endpoints sit alongside the v0.1
   portfolio/activity/approvals views.

## Components added since v0.1

| | |
|---|---|
| **`sherwood-chain`** | read-only JSON-RPC client, Uniswap v4 pool discovery/price reads, `ChainFeed` |
| **`sherwood-signer`** | secp256k1 keys from the vault, sign-local only — no broadcast method exists |
| **`sherwood-wallets`** | named wallets, per-symbol allowlists, per-wallet spend ceilings |
| **`sherwood-dex`** | Uniswap v4 swap calldata construction, Permit2 approvals, `dex-simulate` |
| **`sherwood-router`** | AMM vs. RFQ venue selection by notional — pure decision, no RPC |
| **`sherwood-reconcile`** | reads a receipt for an already-broadcast tx, produces a `Fill` |
| **`sherwood-config`** | `AppConfig` extracted into its own crate so `sherwood-server` can share it |
| **Server API** | `POST /v1/route`, `POST /v1/dex/simulate`, `POST /v1/reconcile`, `GET`/`POST /v1/config`, the ADR-0006 live-mode pre-flight on `POST /v1/mode` |
| **Dashboard** | cash-over-time chart, router / dex-simulate / reconcile cards, alongside the v0.1 views |

## Explicitly not in this release

- **Broadcasting a signed transaction.** `eth_sendRawTransaction` will not be
  added to this codebase, ever — [ADR-0007](adr/0007-no-broadcast-capability.md).
  Sending stays the operator's own tooling, permanently, not deferred.
- **`sherwood-wallets` wired into `sherwood-server`.** Deliberate: nothing
  secret-touching belongs in the network-facing process, even loopback-only —
  see the [2026-09-13 decision log entry](DECISIONS.md#2026-09-13).
- An RFQ venue — none is verified on Robinhood Chain, so every order routes
  to the AMM today. The threshold logic exists so enabling one later is a
  config change, not a control-flow change.
- Dropped from the v0.2 plan entirely: `sherwood-sniper`, `sherwood-copytrade`
  (Solana-memecoin patterns, not tokenised-equity trading).

Full step-by-step detail: [`docs/ROADMAP.md`](ROADMAP.md#v02--robinhood-chain-evm).

## Getting started

```
cp config.example.toml config.toml
export SHERWOOD_VAULT_PASSPHRASE=<yours>
cargo run -p sherwood-cli -- check config.toml
cargo run -p sherwood-cli -- serve config.toml     # API + dashboard on 127.0.0.1:8787
```

Full instructions: [README](../README.md), [DEPLOYMENT.md](DEPLOYMENT.md).

## Operator boundary

Signing with a real key, broadcasting a transaction, and placing the first
live order are the operator's own actions, done with their own tooling. This
project ships no broadcast path and will not perform those steps for you. See
[LIVE_EXECUTION.md](LIVE_EXECUTION.md).
