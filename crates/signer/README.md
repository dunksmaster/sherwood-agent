# sherwood-signer

secp256k1 key custody and EIP-1559 transaction signing for Robinhood Chain
([ADR-0006](../../docs/adr/0006-robinhood-chain-venue.md)).

**Sign-local, broadcast-explicit.** This crate holds a private key, derives
its address, and turns a fully-specified transaction into signed raw bytes.
That is the entire surface — there is no RPC client here and no method that
sends anything to a network. Broadcasting a signed transaction is a separate,
explicit action for a later crate (`sherwood-dex` + live mode) to take.

## Key handling

- The key never comes from a form, argv, or config. It lives in the existing
  `sherwood-secrets` vault: `sherwood secrets set <name>` reads the hex
  private key from **stdin only**.
- `LocalSigner::from_hex` parses it, and its `Debug` impl prints only the
  derived address (`finish_non_exhaustive`) — the key is never printed,
  logged, or returned by any method.
- Decoded key bytes are wrapped in `Zeroizing` and explicitly zeroed after
  use; `k256`'s own key types zeroize on drop.

## CLI

```bash
sherwood secrets set evm_wallet     # paste the hex private key on stdin
sherwood wallet-address evm_wallet  # prints ONLY the derived 0x... address
```

`wallet-address` is what you'd hand a faucet or use to fund the wallet. It
signs nothing and sends nothing.

## What's here

| Module | |
|---|---|
| `rlp` | a minimal RLP encoder — just byte strings, lists, and uints, enough for an EIP-1559 transaction. Known-answer tested against the canonical `"cat"`/`"dog"` RLP examples. |
| `eip1559` | `Eip1559Tx` — the nine unsigned fields, the `0x02`-prefixed signing payload, and the final raw signed transaction |
| (crate root) | `LocalSigner` — load a key, get its address, sign an `Eip1559Tx`; low-`s` normalisation (EIP-2) with the recovery id kept consistent, verified by a sign→recover→address round trip |

## On trusting this with real funds

This crate's cryptography has been checked by **unit tests only**: RLP
known-answer vectors and a self-consistent sign → recover → address round
trip. That is necessary, not sufficient — it proves internal consistency, not
that a real node will accept the transaction the way you expect.

**Before signing anything that moves real value:** sign one transaction,
decode it independently (a block explorer, `cast tx --raw`, or similar), and
confirm the *sender* it reports is exactly the funded wallet. Do this with a
trivial-value transaction first.

## Not here

- Any RPC client (that's `sherwood-chain`).
- Nonce, gas price, or gas limit estimation — `Eip1559Tx` takes them
  fully-specified; fetching live values is the caller's job.
- Swap construction, slippage/deadline bounds — that's `sherwood-dex`
  (v0.2.4).
- A live-mode gate or broadcast path — those arrive together, behind
  `allow_live` + admin + the ADR-0006 pre-flight.
