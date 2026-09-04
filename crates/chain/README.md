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

## CLI

```bash
sherwood chain-probe                     # NVDA on the default RPC
sherwood chain-probe https://rpc… TSLA AAPL
```

Prints a per-token report and exits non-zero if any token's transfers are
restricted or the probe was inconclusive. This is the same check live mode must
run before it arms.

## Next (v0.2.1b)

Uniswap v4 pool reads → a Stock Token price → the existing
`sherwood_core::PriceFeed`, so the paper engine and backtester run on live
on-chain data. No wallet, still all reads. Verified addresses and the
`sqrtPriceX96` → price recipe are in
[`docs/ROBINHOOD-CHAIN.md`](../../docs/ROBINHOOD-CHAIN.md).
