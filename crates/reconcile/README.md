# sherwood-reconcile

Reconciling a transaction the operator already broadcast, against the trade
they expected ([ADR-0007](../../docs/adr/0007-no-broadcast-capability.md)).

`sherwood-dex` builds calldata; the operator simulates it
(`sherwood dex-simulate`), signs it (`sherwood-signer`), and broadcasts it
with their own tooling — **outside this codebase**. What comes back is a
transaction hash. This crate takes that hash and reads its receipt; it never
sends anything, has no signer, and sees no private key.

## What it does

```
reconcile(client, tx_hash, expected) -> Outcome
```

| `Outcome` | When |
|---|---|
| `NotFound` | No receipt yet — still pending, or the hash is unknown to this node |
| `Reverted(receipt)` | Mined, but failed. Gas was spent; nothing to record |
| `Confirmed { receipt, fill }` | Mined and succeeded — `fill` is a `sherwood_core::Fill`, ready for a portfolio or store, same shape a paper fill produces |

`wait_and_reconcile` polls `reconcile` until it stops returning `NotFound` or
a timeout elapses. A timeout is not a verdict on the transaction — it may
still confirm later.

## Trust boundary

A tx hash and an `ExpectedTrade` are trusted no further than the receipt
itself. Reconciliation does not (and cannot) verify the expected trade
against the transaction's calldata — the receipt carries no calldata. Get
the expected trade wrong and the *local* record is wrong; the chain itself
is unaffected either way.

## CLI and server

```
sherwood reconcile <config.toml> <tx_hash> <symbol> <buy|sell> <qty> <price> [fee]
```

Polls for the receipt (up to 2 minutes). On success, if `[general]
state_path` is configured, records the fill exactly the way a paper fill is
recorded: an appended fill, an updated portfolio snapshot, an audit-chain
row.

`sherwood serve` exposes the same check over HTTP as `POST /v1/reconcile`
(operator role, v0.2.10) — same logic, same boundary. One difference: the
HTTP route does not bootstrap a portfolio snapshot the way the CLI command
can (`starting_cash` isn't known to the server) — if none exists yet, it
reports `recorded: false` with a note instead of guessing a starting cash.

## Not here

- Sending anything. `eth_sendRawTransaction` does not appear anywhere in
  this codebase, and per ADR-0007, never will.
- Verifying the transaction's calldata matches what you expected — only its
  outcome (mined, succeeded, gas used).
- Picking up a broadcast automatically. `sherwood reconcile` is a manual,
  after-the-fact command, run once, by hand, after you've broadcast — the
  same way `sherwood dex-simulate` is run once, by hand, before you sign.
