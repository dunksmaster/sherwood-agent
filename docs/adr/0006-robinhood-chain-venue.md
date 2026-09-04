---
status: accepted
date: 2026-09-04
accepted: 2026-09-04
deciders: repository owner
owner-step: v0.2
supersedes-part-of: 0001
---

# ADR-0006 — v0.2 trades on Robinhood Chain (EVM), not Solana

> **Accepted 2026-09-04:** the operator cannot use the Robinhood Agentic Trading
> MCP (US-only; account-opening is geo-gated and the operator is in Albania,
> outside the EU/EEA). v0.1's paper deliverable is unchanged and still ships.
> **v0.2's live venue becomes Robinhood Chain** — Robinhood's permissionless
> Ethereum L2 (chain id `4663`) — traded through its on-chain AMM (Uniswap v4),
> from a self-custody wallet. The planned Solana modules are dropped; their
> shape (RPC abstraction, signer isolation, wallet registry, router) is reused
> against EVM. A read-only on-chain probe
> ([`scripts/rhc-probe.mjs`](../../scripts/rhc-probe.mjs)) verified that Stock
> Token transfers are permissionless at the contract before this was accepted.

## Context

[ADR-0001](0001-mcp-interaction-model.md) assumed the live venue is the Robinhood
Agentic Trading MCP: Robinhood authenticates over OAuth, custodies the assets,
and exposes tool calls that a headless agent makes and the `PreToolUse` hook
gates. That assumption is dead for this operator:

- The Agentic Trading MCP requires a **US individual investing account**.
  Account-opening is geo-gated. The operator is in **Albania**, which is not in
  the US, the EU, or the EEA — so neither the US product nor Robinhood Europe's
  EEA-passported product is available.
- The OAuth flow (`claude mcp add robinhood-trading …`) completes the transport
  handshake but stops at "open an Agentic account".

Robinhood Chain is a different product and a different access model:

| | Agentic Trading MCP | Robinhood Chain |
|---|---|---|
| What | agent places equity orders in a brokerage sub-account | permissionless Ethereum L2 (`4663`), ETH gas, ~100 ms blocks |
| Access | OAuth + US/EEA brokerage account | any EVM wallet + public RPC (`rpc.mainnet.chain.robinhood.com`) |
| Instruments | US equities | **Stock Tokens** — ERC-20s giving economic exposure to NVDA, AAPL, … |
| Venue | Robinhood matching engine | **Uniswap v4** (live from day one) + RFQ |
| Custody | Robinhood | self-custody wallet |
| Available to operator | ❌ | ✅ (subject to the caveats below) |

## Decision drivers

- **v0.1 must not regress.** The paper release is code-complete and tagged. This
  ADR changes only what "live" means, and only for v0.2.
- **Decide on verified facts, not marketing.** "Available in 120 countries" is an
  app-store claim. The question that matters is whether the **token contract**
  lets an un-KYC'd wallet hold and move Stock Tokens. That is checkable on-chain
  and had to be checked before committing.
- **Reuse, don't rewrite.** The v0.2 module shapes already drafted for Solana
  (RPC abstraction, signer isolation, multi-wallet registry with spend ceilings,
  venue router) map onto EVM with the same trait boundaries. Only the chain
  client and the swap-construction code are new.
- **The custody boundary is now the operator's.** Self-custody means a private
  key exists. It lives in the existing `sherwood-secrets` vault (entered on
  stdin, `vault:` reference in config, never logged, never in code). sherwood
  builds and *optionally* signs a transaction; enabling live signing is an
  explicit operator action, exactly as `allow_live` is today.

## Verification (2026-09-04)

`scripts/rhc-probe.mjs` — read-only, signs nothing, sends nothing. Against
mainnet (`chainId 4663`, block ~54.26M):

- **Stock Tokens are beacon proxies.** Each token (e.g. NVDA
  `0xd060…9EEC`, TSLA `0x322F…3b2d`) is a 283-byte proxy over one shared
  implementation `0xb354…5ae2` via beacon
  `0xe10b6f6b275de231345c20d14ab812db62151b00`.
- **Transfers are permissionless at the contract.** Simulated
  `transfer(<fresh un-KYC'd address>, amount)` from a funded holder returns
  `true`. The reverse (`transfer` *from* the fresh address) reverts
  `ERC20InsufficientBalance` — i.e. failure is balance-gated only, not
  identity-gated. No allowlist, no compliance hook, `paused() == false`.
- **Recent traffic is real and organic.** ~1300 NVDA `Transfer` events in a
  1500-block window, 100 % wallet↔wallet, hundreds of distinct addresses.
- **Liquidity is real.** The dominant contract holder
  (`0x8366…0951`, a multi-asset AMM) custodies ~22k NVDA + ~1.7k WETH + ~48M
  USDG — Stock Tokens are quotable and tradeable on-chain now.

### Caveats carried into v0.2 (documented, not blocking)

1. **Upgradeable implementation.** One beacon controls the logic of *every*
   Stock Token. A future implementation could add a transfer allowlist and
   freeze un-KYC'd wallets. sherwood must re-run the probe as a pre-flight check
   and refuse live mode if `transfer` to a fresh address stops simulating clean.
2. **Sequencer-level screening.** Robinhood Chain screens transactions at the
   sequencer against a sanctions list. A sanctioned address is blocked
   chain-wide regardless of the token contract. Not a jurisdiction gate for an
   ordinary wallet, but it means liveness depends on Robinhood's single
   sequencer.
3. **No primary-market access.** Minting new Stock Tokens and redeeming them for
   the underlying shares/cash needs issuer KYC (EEA only). The operator can hold
   and trade the token, not redeem it. Economic exposure, not ownership.
4. **Gas is ETH on an L2.** The wallet needs bridged ETH; a bridge route and its
   risks are the operator's.
5. **Tax and regulatory treatment** of holding tokenised US equities from
   Albania is the operator's responsibility. sherwood gives no advice here.

## Decision

1. **v0.1 (paper) is unchanged and ships as tagged.** ADR-0001's hook model
   stays the documented path for anyone who *does* have the Agentic MCP.
2. **v0.2's live venue is Robinhood Chain.** New/renamed modules:

   | Module | Was (Solana) | Now (Robinhood Chain / EVM) |
   |---|---|---|
   | `sherwood-chain` | Solana RPC abstraction | EVM JSON-RPC client (`alloy`), chain `4663`, nonce/gas/receipt handling |
   | `sherwood-signer` | custody tiers, signer isolation | same shape; secp256k1 keys from the vault; sign-locally, broadcast-explicitly |
   | `sherwood-wallets` | multi-wallet registry, spend ceilings | unchanged in shape; EVM addresses |
   | `sherwood-dex` | (was `sherwood-sniper` + `-copytrade`) | Uniswap v4 quote + swap construction on Robinhood Chain; `slippage`/`deadline`/`minOut` |
   | `sherwood-router` | venue selection | AMM vs RFQ selection by notional |

   Copy-trade and sniper are **dropped from v0.2** — they were Solana-memecoin
   patterns. They may return post-v0.2 as EVM equivalents if wanted.
3. **The paper path gains a real data source first.** `sherwood-chain`'s read
   half (RPC connect, read Uniswap v4 pool state, derive a Stock Token price)
   feeds the existing `PriceFeed` trait. This is pure reads — no wallet, no
   signing — and is the first v0.2 work item. It also makes the backtester run
   on real on-chain history instead of a CSV.
4. **Live execution stays gated exactly as today.** `allow_live = true` +
   admin + explicit toggle, plus a mandatory pre-flight: `rhc-probe`-style check
   that `transfer` to a fresh address still simulates clean, or live mode
   refuses to arm.
5. **The `RiskGate`, approval gate, session budgets, audit chain, server,
   dashboard, and CLI are venue-agnostic and unchanged.**

## Consequences

- ADR-0001 is **partly superseded**: the "Robinhood MCP" venue becomes optional,
  and a second venue adapter (EVM) sits behind the same `Executor` trait. The
  hook model itself is untouched.
- A new dependency surface: `alloy` (MIT/Apache-2.0) for EVM RPC + signing +
  ABI. To be added to `deny.toml` review when the crate lands, not before.
- The threat model gains an entry: **key custody**. v0.1 had no private keys;
  v0.2 does. `THREAT-MODEL.md` needs a signing-path section before live code.
- "Paper" now has two meanings worth keeping distinct: *simulated fills against
  real on-chain prices* (new, high-fidelity) vs *simulated fills against a CSV*
  (existing). Both stay.
- `hoodmap` (the ecosystem directory repo) becomes directly relevant — it can
  catalogue Robinhood Chain Stock Token addresses and pools that `sherwood-dex`
  consumes.

## Alternatives considered

- **Generic EVM DEX (Base / Arbitrum + Uniswap).** Fully permissionless, deep
  liquidity, no Robinhood dependency. Rejected as the *default* because the
  operator's stated interest is tokenised-equity exposure, which is Robinhood
  Chain's specific offering. The `sherwood-chain` client is written chain-id-
  agnostic, so pointing it at another EVM chain later is a config change.
- **Wait for Robinhood to open the Agentic MCP to more countries.** Indefinite;
  no announced timeline for Albania.
- **Keep the Solana plan.** Solana is permissionless and liquid, but it does not
  have Robinhood's tokenised equities and would be a genuinely different product.
- **Drop live entirely, ship paper-only forever.** Viable, but the operator
  explicitly wants a path to live; this ADR gives one that actually works from
  their jurisdiction.
