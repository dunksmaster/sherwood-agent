# sherwood-dex

Uniswap v4 swap construction on Robinhood Chain
([ADR-0006](../../docs/adr/0006-robinhood-chain-venue.md)).

Same boundary as every crate below it in the v0.2 stack: **no RPC client, no
method that broadcasts anything.** This crate builds calldata for a
single-hop exact-input `V4_SWAP` through the `UniversalRouter`, plus the two
Permit2 approvals it depends on. It can, via `sherwood-signer`, sign the
transaction that calldata belongs to — signing is still not sending.

## What's here

| Module | |
|---|---|
| `v4swap` | `ExactInputSingleSwap::execute_calldata` — the full `UniversalRouter.execute(bytes,bytes[],uint256)` calldata for one swap |
| `permit2` | `erc20_approve_calldata`, `permit2_approve_calldata` — the two prerequisite approvals (bounded amounts, no "unlimited" approve) |
| `quote` | `amount_out_minimum` (slippage bound) and `deadline_from_now` — pure math, no chain access |
| `abi_dyn` | the dynamic-type ABI encoding (`bytes`, `bytes[]`, a struct with a dynamic field) the swap calldata needs, beyond `sherwood_chain::abi`'s static-word helpers |

## Where the byte layout came from

Every command byte, action byte, and struct field order in `v4swap` is
sourced from `Uniswap/universal-router`'s `Commands.sol` and
`Uniswap/v4-periphery`'s `Actions.sol` / `IV4Router.sol` / `V4Router.sol` /
`CalldataDecoder.sol` — not reconstructed from memory. The strongest
evidence it's right: this encoder's output length for
`ExactInputSingleParams` (with empty `hookData`) is **exactly** `0x160`
bytes, and for a `(currency, uint256)` pair **exactly** `0x40` bytes —
matching `CalldataDecoder`'s own minimum-length assembly checks byte for
byte. `minHopPriceX36` is set to `0`, which the router treats as "disabled"
— `amountOutMinimum` is this swap's real slippage bound.

## Verification status — read this before trusting it with real value

- **Structural correctness: confirmed, and one bug found and fixed.** A real,
  currently-successful (`status: 0x1`) `UniversalRouter.execute` transaction was pulled
  from the live chain and diffed word-by-word against this crate's output for an
  equivalent swap. Every framing word matched (selector, the three offset words, the
  `060c0f` action sequence, the `ExactInputSingleParams` layout, the `SETTLE_ALL` /
  `TAKE_ALL` param shape) **except one**: the `hookData` offset inside
  `ExactInputSingleParams`. This crate emitted `9*32` (`0x120`); the real transaction
  had `10*32` (`0x140`). Ten head words precede `hookData` — the five `PoolKey` words,
  `zeroForOne`, `amountIn`, `amountOutMinimum`, `minHopPriceX36`, and the offset word
  itself. At `0x120` the offset points at itself, and `v4-periphery`'s `CalldataDecoder`
  slices `hookData` out of bounds and does a bare `revert(0, 0)` inside
  `PoolManager.unlock` → `UniversalRouter.unlockCallback` — the empty-data (`0x`) revert
  that made *every* simulated swap fail. Fixed in `v4swap.rs` (now `10*32`), with a unit
  regression test pinning the word to `0x140`.
- **End-to-end simulation of a Stock Token pool: now succeeds.** After the fix,
  `sherwood dex-simulate` for a 5 USDG → NVDA single-hop swap returns success via
  `eth_call`, from a wallet confirmed at call time to hold USDG and to have both
  prerequisite approvals set. The pool it runs against is the one `find_best_pool`
  selects: NVDA/USDG, dynamic fee (`0x800000`), tickSpacing 10, hook
  `0x66622f77…` — the hook was never the problem; passing it empty `hookData` is fine.
- **Local trace, no paid RPC.** The public endpoint has no `debug_traceCall`, so the bug
  was localized with a Foundry fork replay — `debug/foundry/`, one dependency-free test
  file. It forks Robinhood Chain, pranks the same wallet the live sim uses, and replays
  the exact `execute` calldata: the pre-fix bytes revert with empty data inside
  `unlockCallback` (before the pool is touched), the post-fix bytes simulate clean.
  `forge test --fork-url https://rpc.mainnet.chain.robinhood.com -vv`.

**Practical conclusion: `sherwood dex-simulate` for the exact pool you intend to trade
is the gate — it caught this bug, which is what it is for. Do not sign or broadcast a
swap built by this crate until that simulation returns success for that pool, from your
actual funded, Permit2-approved wallet.** Before signing anything for real:

1. Build the swap.
2. `sherwood dex-simulate <from> <token> <amount_raw> [denom] [bps]` —
   `eth_call`-simulates the exact calldata this crate produces. Costs
   nothing, changes nothing on chain. `sherwood serve` exposes the same
   check over HTTP as `POST /v1/dex/simulate` (operator role, v0.2.9) —
   same logic, same boundary, just callable from the dashboard instead of a
   terminal.
3. If it reverts, do not proceed — investigate (a debug-trace-capable RPC would
   pin down the exact failing step; this session didn't have one available).
4. Only sign for real once that simulation succeeds from your actual funded,
   Permit2-approved wallet, against the actual pool you intend to trade.

## Prerequisites for a swap to actually succeed

`SETTLE_ALL` pays the input token from the caller via **Permit2**, not a
plain `ERC20.transferFrom`. Before any swap can settle:

1. `ERC20.approve(PERMIT2_ADDRESS, amount)` on the input token.
2. `Permit2.approve(token, UNIVERSAL_ROUTER, amount, expiration)`.

Both calldata builders are in `permit2.rs`. Building, signing, and
broadcasting these two prerequisite transactions is the operator's job —
nothing here does it automatically.

## Not here

- Sending anything — `eth_sendRawTransaction` does not appear anywhere in
  this codebase.
- Multi-hop swaps, exact-output swaps, native-ETH legs — only single-hop
  exact-input, ERC-20 to ERC-20.
- Picking a wallet or checking its spend ceiling — that's `sherwood-wallets`
  (`Wallet::try_reserve` before building the swap, `Wallet::signer()` after).
- A live-mode gate — v0.2.6, gated behind `allow_live` + admin + the
  ADR-0006 pre-flight (`sherwood chain-probe`).
