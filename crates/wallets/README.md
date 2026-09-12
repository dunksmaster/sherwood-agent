# sherwood-wallets

A multi-wallet registry for Robinhood Chain
([ADR-0006](../../docs/adr/0006-robinhood-chain-venue.md)).

Same boundary as `sherwood-signer`, one layer up: **no RPC client, no
broadcast method anywhere**. A [`Wallet`] wraps a `sherwood-signer`
[`LocalSigner`](../signer/README.md), and that only signs.

## What a wallet is

| Field | |
|---|---|
| `name` | how strategies/config refer to it |
| key | a `vault:NAME` reference, resolved through `sherwood-secrets` — same pattern as `[ai] api_key`. Never a literal in config. |
| `allowed_symbols` | which symbols this wallet may trade; empty = unrestricted |
| a spend ceiling | tx count / cumulative notional / duration — the same hard-stop-and-latch shape as `sherwood-server`'s per-session budget, scoped to this one wallet |

`WalletRegistry::load` is all-or-nothing: a missing secret, a duplicate
name, or bad key material fails the whole load rather than silently
skipping a wallet — a wallet a strategy expected to have and doesn't is a
capability loss, not something to shrug past.

## CLI

```bash
sherwood secrets set evm_wallet     # paste the hex private key on stdin
sherwood wallets config.toml        # load [[wallets]] and print name/address/budget
```

Never prints a key. See `config.example.toml` for the `[[wallets]]` schema.

## Not here

- Any RPC client, or a way to actually place a trade (`sherwood-dex`,
  v0.2.4).
- Signing or sending anything. `sherwood run` calls `wallet_for_symbol` per
  fill (v0.2.6 runner integration) to log which wallet a live order would
  have used, alongside `sherwood-router`'s venue choice — logging only, no
  calldata built, no RPC call. Actually building and signing a swap is still
  a separate, explicit step (`sherwood dex-simulate`, then
  `sherwood-signer`).
