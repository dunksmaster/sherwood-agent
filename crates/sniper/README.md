# sherwood-sniper

New-pool event screening for the v0.2 Solana milestone.

**Status: real, tested, and intentionally not wired.** This crate contains working logic —
`RugScreen` runs seven concrete checks (initial liquidity, deployer supply fraction, LP lock
presence and duration, mint authority, freeze authority, buy/sell tax), and `entry_order`
builds a sized order only when the screen is clean. Four unit tests cover the pass path and
each failure flag.

It is **not** reachable from the `sherwood` binary. Sniping requires an on-chain pool-event
source and a signer, both of which land in v0.2 — see
[`docs/ROADMAP.md`](../../docs/ROADMAP.md#v02--solana-modules). Until then this is library
code kept for design continuity, not a stub and not dead code
([`docs/DEFINITION-OF-DONE.md`](../../docs/DEFINITION-OF-DONE.md#the-current-known-instance)).
