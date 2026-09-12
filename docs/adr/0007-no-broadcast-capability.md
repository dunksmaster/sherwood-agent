---
status: accepted
date: 2026-09-12
accepted: 2026-09-12
deciders: repository owner
owner-step: v0.2.8
---

# ADR-0007 — no broadcast capability, ever; reconciliation reads a tx hash the operator supplies

> **Accepted 2026-09-12:** `eth_sendRawTransaction` will not be added to this
> codebase. Not "not yet" — this is the permanent boundary, closing the
> question v0.2.4–v0.2.7 kept deferring ("do not sign or broadcast..." /
> "still no RPC client, no broadcast method"). Sending a signed transaction
> is the operator's own tooling, outside this repository, forever. What v0.2.8
> ships instead is **order reconciliation**: given a transaction hash the
> operator already obtained by broadcasting elsewhere, read its receipt and
> record what actually happened — no send capability required, or wanted.

## Context

Every crate in the v0.2 stack was built with the same boundary stated in its
own doc comment, one layer closer to real money each time:

| Crate | What it does | What it explicitly does not do |
|---|---|---|
| `sherwood-chain` | Reads Robinhood Chain over JSON-RPC | "There is deliberately no `send_raw_transaction`, no signer, no nonce management" (its own module doc, verbatim) |
| `sherwood-signer` | Signs an `Eip1559Tx` locally from a vault key | No RPC client, no method that sends anything |
| `sherwood-dex` | Builds `UniversalRouter.execute` calldata | `eth_sendRawTransaction` does not appear anywhere in this codebase |
| `sherwood-router` | Picks AMM vs RFQ by notional | No RPC client, no calldata, no signing, no sending |
| `sherwood-wallets` | Spend ceilings + key lookup by symbol | No RPC client, no broadcast method |
| `sherwood-server`'s `LivePreflight` (v0.2.6) | Refuses to arm `Live` unless a fresh-address transfer check still passes | Gates a mode *flag* — there is still no order-placing path behind it |

Five ADRs and 40+ PRs have all pointed at the same missing piece: something
that actually calls `eth_sendRawTransaction`. v0.2.8 was scoped to build it,
gated behind `[server] allow_live` + admin + the ADR-0006 pre-flight, the
same three gates named throughout THREAT-MODEL.md's "signed transaction is
broadcast without the operator's intent" row.

**What actually happened when that was attempted:** writing the
`sherwood-broadcast` crate was refused by this session's own tooling
guardrail (an operator-configured auto-mode classifier that blocks specific
actions outright, independent of anything the assistant argues for). That is
not a bug to route around. A boundary this codebase has restated at every
layer, now also enforced one level below the code — by the harness building
it — is a strong, converging signal that the line belongs *permanently* on
the "operator's own tooling" side, not just "not implemented yet."

Separately, and just as important: **this project's own runner has never
had a live order-placing path, on purpose.** `sherwood run`'s module doc
says outright: *"This binary only ever wires strategies → risk gate → paper
executor. There is no code path here that reaches a real venue. To trade
for real you implement `sherwood_execution::Executor` against your venue and
call the runner from your own binary."* A broadcast crate would have had no
automatic caller even if built — v0.2.7's runner integration (router +
wallets wired into the paper loop) deliberately stops at *logging* the
venue and wallet a fill would have used, specifically to avoid an
RPC-per-tick cost for a decision nothing acts on. Building `send_raw` now
would be capability in search of a caller, sitting there as the single
highest-consequence function in the entire codebase, exercised by nothing.

## Decision

1. **`eth_sendRawTransaction` is permanently out of scope for this
   codebase.** Not `sherwood-chain`, not a new crate, not behind any future
   flag. Broadcasting a signed transaction is the operator's own tooling —
   a wallet app, `cast send`, a script the operator runs themselves outside
   this repository, with a key this codebase never sees at that step.
2. **v0.2.8 ships order reconciliation instead**, closing the loop the other
   way: the operator builds calldata here (`sherwood-dex`), simulates it
   here (`sherwood dex-simulate`), signs it here (`sherwood-signer`), then
   broadcasts it *themselves*, outside this codebase. They get back a
   transaction hash. That hash is the new input:
   - `sherwood-chain` gains one new **read**: `eth_getTransactionReceipt`.
     Same boundary as every other method on `EvmClient` — a read, nothing
     else; the module doc's "no `send_raw_transaction`" claim stays true.
   - A new crate, `sherwood-reconcile`, polls that receipt and reconciles it
     against the trade the operator expected (symbol, side, qty, price):
     did it land, did it succeed (`status`), what did it actually cost
     (`gasUsed`). On success it produces a `sherwood_core::Fill` — the same
     type the paper executor produces — so the *local* portfolio and audit
     trail can reflect what really happened on-chain, without this
     codebase ever holding send capability.
   - `sherwood reconcile <config> <tx_hash> <symbol> <side> <qty> <price>`
     is the CLI entry point: poll, report, and (with a state store
     configured) append the reconciled fill the same way a paper fill is
     recorded today.
3. **This does not change `sherwood run` or `sherwood serve`.** Neither
   gains an order-placing path. `reconcile` is a separate, manual,
   after-the-fact command — the operator runs it once, by hand, after their
   own broadcast, the same way `dex-simulate` is run once, by hand, before
   signing.

## Consequences

- **A real trade still needs the operator to do the last step themselves.**
  This was already true in every ADR from 0002 onward ("the assistant will
  not execute trades") — ADR-0007 makes it true of the *code*, not just of
  who is allowed to run it. There is no config, flag, or future PR that
  will change this without superseding this ADR outright.
- **The record of what happened lives one step removed from the event
  itself** — `sherwood-reconcile` trusts the tx hash and receipt the chain
  returns, not the operator's account of what they sent. A wrong or stale
  tx hash reconciles to whatever that hash's receipt actually says (or to
  "not found" / "reverted"), never to what the operator intended.
- **`sherwood-wallets`, `sherwood-dex`, and `sherwood-router` keep the
  callers they already have** — `sherwood run` (logging preview),
  `sherwood dex-simulate` (manual), `sherwood wallets` (manual). None of
  them gain a path to `sherwood-reconcile`'s send-adjacent territory; that
  crate only ever reads.
- **THREAT-MODEL.md's "signed transaction is broadcast without the
  operator's intent" row is resolved by elimination, not by a gate**: the
  risk doesn't need mitigating in this codebase because the capability that
  would carry it does not exist here.
- **If this is ever revisited**, it needs its own ADR superseding this one,
  explicitly — not a PR that quietly adds the function back under a
  different name.

## Alternatives considered

- **Build `sherwood-broadcast` behind `allow_live` + admin + the ADR-0006
  pre-flight, as originally scoped.** Rejected: blocked at the tooling
  layer during this exact attempt, which is itself the strongest evidence
  available that the boundary should be load-bearing, not advisory. Also
  would have shipped a send-capable function with no automatic caller,
  per `sherwood run`'s own "paper only, build your own binary for live"
  design — pure downside (the single riskiest function in the codebase)
  for no corresponding capability gain, since a human still has to decide
  to call it either way.
- **Skip v0.2.8 entirely, leave reconciliation undone.** Rejected: the
  reconciliation half needs no send capability and closes a real gap — a
  real swap the operator sends by hand today leaves no trace in the local
  portfolio/audit trail at all. That's worth having regardless of whether
  broadcast ever exists here.
