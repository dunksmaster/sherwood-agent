# debug/foundry — V4_SWAP revert fork test

A one-file [Foundry](https://getfoundry.sh) harness used to root-cause the
`sherwood-dex` swap revert. **Not part of the Rust workspace or CI** — it
needs `forge` and network access to a Robinhood Chain RPC, neither of which
CI has. Kept in-tree because it documents, reproducibly, why v0.2.4's
simulated swaps reverted.

## What it found

`ExactInputSingleParams` encoded its `hookData` offset as `9*32` (`0x120`)
instead of `10*32` (`0x140`). Ten head words precede `hookData`: the five
`PoolKey` words, `zeroForOne`, `amountIn`, `amountOutMinimum`,
`minHopPriceX36`, and the offset word itself. At `0x120` the offset points
at itself; `v4-periphery`'s `CalldataDecoder` then slices `hookData` out of
bounds and does a bare `revert(0, 0)` inside `PoolManager.unlock` →
`UniversalRouter.unlockCallback`. That is the empty-data (`0x`) revert that
made every simulated swap fail.

Fixed in `crates/dex/src/v4swap.rs`.

## Run it

```
cd debug/foundry
forge test --fork-url https://rpc.mainnet.chain.robinhood.com -vvvv
```

- `test_buggy_offset_reverts_empty` — replays the pre-fix calldata; asserts
  it reverts with zero-length data inside `unlockCallback`.
- `test_fixed_calldata_succeeds` — replays the exact calldata `sherwood-dex`
  emits after the fix (5 USDG to NVDA); asserts it simulates clean.

Both pull the same wallet the live `sherwood dex-simulate` uses
(`0x6031775d...`, funded + Permit2-approved for USDG) and fork at the block
that simulation ran on. The `test/calldata_good.hex` and
`test/calldata_buggy.hex` blobs came straight from `sherwood dex-simulate`
output (buggy = the same bytes with the offset word reverted to `0x120`).

## Provenance

Reference successful transaction diffed against:
`0xe9655a4080436fccd158057cfafab080f247238e6fafa647902e60250fc36e13`
(`UniversalRouter.execute`, `status 0x1`), whose `hookData` offset word is
`0x140`.
