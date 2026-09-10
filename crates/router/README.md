# sherwood-router

Venue selection for Robinhood Chain
([ADR-0006](../../docs/adr/0006-robinhood-chain-venue.md)).

One question: given an order's notional, does it route to the **AMM** (Uniswap
v4, built by [`sherwood-dex`](../dex/README.md)) or to an **RFQ** venue?
`Router::choose` answers it and returns a `RouteChoice` — the venue plus a
human-readable reason naming the numbers it compared, for the audit log.

Same boundary as every crate below it in the v0.2 stack: **no RPC client, no
calldata, no signing, no sending.** Pure decision logic.

## RFQ is not wired

ADR-0006 lists "Uniswap v4 + RFQ" as Robinhood Chain's venues, but no RFQ
endpoint or contract has been verified, and nothing in this codebase talks to
one. So `RouterConfig::rfq_available` is `false` in every real config today and
**every order routes to the AMM**. The threshold logic exists now so that
integrating an RFQ client later is a config change (`rfq_available: true` + a
`rfq_min_notional`), not a control-flow change here.

## Decision table

| `rfq_min_notional` | `rfq_available` | notional | → |
|---|---|---|---|
| unset | `false` | any | AMM ("no RFQ threshold configured") |
| set | `false` | any | AMM ("no RFQ venue is configured — falling back") |
| set = T | `true` | `< T` | AMM |
| set = T | `true` | `>= T` | RFQ (boundary inclusive) |

`Router::new` rejects a non-positive threshold, and rejects `rfq_available`
with no threshold (that config can never pick RFQ — write `rfq_available:
false` if AMM-only is the intent). `choose` rejects a non-positive notional.

## CLI

```
sherwood route <notional> [rfq_min_notional] [rfq_available]
```

Prints the venue and the reason. `rfq_available` is any of `1｜true｜yes` /
`0｜false｜no` (default `false`). Examples:

```
sherwood route 500                    # -> AMM (no threshold)
sherwood route 50000 10000            # -> AMM (threshold set, no RFQ venue)
sherwood route 50000 10000 true       # -> RFQ
sherwood route 9999 10000 true        # -> AMM (below threshold)
```

## Not here

- Any RFQ integration — see above.
- Building the AMM swap once `AMM` is chosen — that's `sherwood-dex`
  (`ExactInputSingleSwap`).
- Picking the wallet / checking its spend ceiling — that's `sherwood-wallets`.
- Wiring routing into the `sherwood run` order flow — a later step; nothing
  calls `Router::choose` in anger yet.
