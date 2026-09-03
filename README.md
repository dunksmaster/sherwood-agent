# sherwood-agent

A Rust workspace for building automated trading strategies — **copy trading**,
**sniping**, and an **AI decision layer** — with a hard risk gate in front of
every order.

> **Paper trading only, out of the box.** The `sherwood` binary never touches a
> real venue. It wires strategies → risk gate → a fill *simulator*. Trading real
> money requires you to implement a live execution adapter yourself, hold your
> own keys, and accept your venue's agreements. See
> [`docs/LIVE_EXECUTION.md`](docs/LIVE_EXECUTION.md).

## Why it's split this way

```
          ┌─────────────┐   ┌──────────────┐
strategies│  copytrade  │   │    sniper    │  ── emit Orders
          └──────┬──────┘   └──────┬───────┘
                 └────────┬────────┘
                   ┌──────▼──────┐
   decision layer  │  decision   │  ── Buy / Sell / Hold (rule or AI)
                   └──────┬──────┘
                   ┌──────▼──────┐
   the choke point │  RiskGate   │  ── notional / position / daily-loss /
                   └──────┬──────┘     slippage / allow-deny / kill switch
                   ┌──────▼──────┐
   execution seam  │  Executor   │  ── PaperExecutor (default) │ LiveExecutor (stub)
                   └─────────────┘
```

Every strategy funnels through the **same** `RiskGate`. A misbehaving strategy
or AI model can only do as much damage as the gate allows.

## Crates

| Crate | Responsibility |
|-------|----------------|
| `sherwood-core` | Domain types, `Portfolio` ledger, `RiskGate`. No I/O. |
| `sherwood-execution` | `Executor` trait, `PaperExecutor`, `LiveExecutor` (stub). |
| `sherwood-copytrade` | Translate observed leader trades → sized `Order`s. |
| `sherwood-sniper` | New-pool events + `RugScreen` safety checks → entry `Order`. |
| `sherwood-decision` | `Decider` trait, `RuleDecider`, `AiDecider` (wraps your LLM call). |
| `sherwood-cli` | `sherwood` binary — paper runner only. |

## Quick start

```bash
cargo test
cargo run -p sherwood-cli -- demo
cp config.example.toml config.toml
cargo run -p sherwood-cli -- run config.toml
```

## The AI decision layer

`AiDecider` takes an `async` closure that *you* write. That closure owns the API
client (e.g. the Claude API), the prompt, and the parsing of the reply into a
`Decision`. This crate ships no prompt that recommends specific assets and makes
no network calls of its own. Whatever the model returns is still clamped by the
risk gate.

## Status

Early scaffold. Strategy translation, risk gate, portfolio ledger, and the paper
executor are implemented and tested. Live data feeds and the live execution
adapter are intentionally not included.

## License

MIT
