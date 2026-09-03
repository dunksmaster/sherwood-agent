# sherwood-copytrade

Leader-trade mirroring for the v0.2 Solana milestone.

**Status: real, tested, and intentionally not wired.** `CopyTrader::mirror` translates an
observed leader trade into a sized `Order` under three sizing modes (fixed fraction, fixed
notional, proportional-to-equity), clamps sells to the held quantity, caps mirror notional,
and filters unknown leaders and dust. Five unit tests cover each mode and each skip reason.

It is **not** reachable from the `sherwood` binary. Copy-trading requires a live
`TradeFeed` — a wallet-log subscription and swap decoder — which lands in v0.2, see
[`docs/ROADMAP.md`](../../docs/ROADMAP.md#v02--solana-modules). Until then this is library
code kept for design continuity, not a stub and not dead code
([`docs/DEFINITION-OF-DONE.md`](../../docs/DEFINITION-OF-DONE.md#the-current-known-instance)).
