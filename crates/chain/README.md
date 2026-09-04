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
| `univ4` | [`read_price`] — discover a token's Uniswap v4 pools, rank by liquidity, and convert the deepest pool's `sqrtPriceX96` into a `Decimal` price |

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

## Next (v0.2.1c)

Wire `univ4::read_price` into `sherwood_core::PriceFeed` (config-selectable in
`sherwood run` / `sherwood backtest`), so the paper engine and backtester run
on live on-chain data. `PriceFeed::next_tick` is synchronous — the feed bridges
its async reads via `tokio::task::block_in_place` + `Handle::block_on`, per
`sherwood_core::feed`'s doc comment on how a live feed is expected to fit the
trait. Still no wallet, still all reads. Verified addresses and the
`sqrtPriceX96` → price recipe are in
[`docs/ROBINHOOD-CHAIN.md`](../../docs/ROBINHOOD-CHAIN.md).
