# sherwood-chain

Read-only EVM JSON-RPC client for Robinhood Chain
([ADR-0006](../../docs/adr/0006-robinhood-chain-venue.md)).

This crate does I/O but **never builds, signs, or sends a transaction**. Every
method is an `eth_call` / `eth_getLogs` / `eth_getCode` / `eth_getStorageAt`
read. There is no method that takes a private key. Transaction construction and
signing are later crates (`sherwood-signer`, `sherwood-dex`), gated separately.

## What's here

| Module | |
|---|---|
| `rpc` | [`EvmClient`] trait (one required method, `request`) + read helpers; [`HttpClient`], the `reqwest` implementation |
| `keccak` | `keccak256`, function selectors, event topics — with known-answer tests |
| `abi` | the sliver of ABI coding the reads need: pad an address/uint into a word; decode `uint256` / `address` / `string` |
| `erc20` | [`Erc20`] — `name` / `symbol` / `decimals` / `totalSupply` / `balanceOf`, scaled to `Decimal` |
| `probe` | [`check_transfer_open`] — the ADR-0006 pre-flight: simulate a transfer to a fresh un-onboarded address and check the token is not allowlist-gated |
| `univ4` | [`read_price`] — discover the pool between an exact `(token, denom)` pair, rank candidates by liquidity, and convert the deepest pool's `sqrtPriceX96` into a `Decimal` price |
| `feed` | [`ChainFeed`](src/feed.rs) — a `sherwood_core::PriceFeed` over live `univ4` reads, for `sherwood run`'s `[chain]` config |
| `tokens` | known Stock Token / stable addresses and the Uniswap v4 deployment, shared by the CLI commands and `ChainFeed` |

## CLI

```bash
sherwood chain-probe                     # NVDA on the default RPC
sherwood chain-probe https://rpc… TSLA AAPL

sherwood chain-price                     # NVDA priced in USDG
sherwood chain-price https://rpc… TSLA WETH
```

`chain-probe` exits non-zero if any token's transfers are restricted or the
probe was inconclusive — the same check live mode must run before it arms.
`chain-price` prints the deepest pool found for a token and its price.

`EvmClient::get_logs` bisects and retries a block range the node refuses (too
wide, or it timed out) rather than failing outright — pool discovery scans a
token's whole history by filtering on an indexed `Initialize` topic, which can
still be a wide range on an RPC that caps `eth_getLogs`.

## Live prices in `sherwood run`

Set `[chain] enabled = true` in `config.toml` (see `config.example.toml`) and
`sherwood run` polls real Uniswap v4 prices instead of a CSV/the demo feed —
**still paper trading**, the feed only supplies prices. Pool discovery runs
once per symbol and is cached; every later poll is one cheap `getSlot0`.
`PriceFeed::next_tick` is synchronous, so `ChainFeed` bridges its async reads
via `tokio::task::block_in_place` + `Handle::block_on`. A failing read retries
with backoff and, past a cap, sleeps a full poll interval rather than
returning `None` (a live feed must never return `None`) or spinning forever.

Not wired into `sherwood backtest` — a live poller isn't a bounded replay.

**A real bug this caught:** the first version discovered *any* pool
containing the token, regardless of its counter-currency, and assumed that
currency was the requested denominator — so the deepest pool by liquidity
turned out to be paired against something else entirely, priced as if it were
`token`/`denom`, off by roughly 10¹²x. Fixed by constraining discovery to the
exact `(token, denom)` pair (`univ4::discover_pools_for_pair`); covered by a
regression test. Verified addresses and the `sqrtPriceX96` → price recipe are
in [`docs/ROBINHOOD-CHAIN.md`](../../docs/ROBINHOOD-CHAIN.md).
