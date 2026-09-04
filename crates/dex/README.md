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

Two different checks were run, with two different results:

- **Structural correctness: confirmed.** A real, currently-successful (`status: 0x1`)
  `UniversalRouter.execute` transaction was pulled from the live chain and diffed
  byte-for-byte against this crate's own output for an equivalent swap shape. Every
  field — the selector, the three offset words, the `060c0f` action sequence, the
  `ExactInputSingleParams` layout (including `minHopPriceX36` at exactly byte 288, and
  `hookData`'s offset at exactly `9*32`), and the `SETTLE_ALL`/`TAKE_ALL` param shape —
  matched exactly. This is strong evidence the encoding logic in `v4swap.rs` is right.
- **End-to-end simulation of a Stock Token pool: not yet successful — narrowed, not
  solved.** `sherwood dex-simulate` against the live NVDA/USDG pool reverts with empty
  revert data (`0x`), from a wallet confirmed — freshly, at call time — to hold ample
  USDG and to have both prerequisite approvals already maxed out. What's been ruled out,
  each checked directly against the live chain:
  - **Not the RPC plumbing.** Re-simulating an *actual* successful transaction from its
    real sender succeeds cleanly via the same `eth_call` path.
  - **Not "wrong pool by liquidity ranking."** `NVDA/USDG` has 220+ Initialize'd pools
    (most look like spam/dynamic-fee decoys). The one `find_best_pool` selects (fee
    3000, tickSpacing 60) has ~78× the raw `getLiquidity()` of the pool at the fee tier
    (375, tickSpacing 4) that a *different* Stock Token/USDG pair's real successful swap
    actually used. Tried both — **both revert identically.**
  - **Not Permit2 or the SETTLE step.** `Permit2.transferFrom(wallet → router, 5 USDG)`,
    called directly (bypassing the router/pool entirely) with `from` set to the router
    (the approved spender), **succeeds** — the wallet's real USDG moves.
  - **Not the TAKE step either.** `NVDA.transfer(recipient, 1)` called with `from` set to
    the `PoolManager` itself **succeeds** — consistent with ADR-0006's finding that NVDA
    transfers are permissionless.

  That leaves the `SWAP_EXACT_IN_SINGLE` action's actual pool-swap execution (inside
  `V4Router`/`PoolManager.unlock`) as the remaining suspect, isolated by elimination
  rather than confirmed directly — a debug-trace-capable RPC (`debug_traceCall`, not
  available on the public endpoint used here) would settle it outright.

**Practical conclusion: do not sign or broadcast a swap built by this crate against a
real pool until `sherwood dex-simulate` for that exact pool returns success.** That
command is exactly the gate to use — it caught this open question, which is what it is
for. Before signing anything for real:

1. Build the swap.
2. `sherwood dex-simulate <from> <token> <amount_raw> [denom] [bps]` —
   `eth_call`-simulates the exact calldata this crate produces. Costs
   nothing, changes nothing on chain.
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
