# sherwood-agent

A Rust workspace for automated trading with a hard risk gate in front of every order.

> **Status: planning + scaffold.** No live venue is wired. The `sherwood` binary refuses any
> mode that isn't `paper`, and `LiveExecutor` is a stub that never fills. See
> [`docs/CURRENT-STATE.md`](docs/CURRENT-STATE.md) for an honest account of what is real
> versus what is a placeholder.

## Scope

| Milestone | Venue | What it does |
|---|---|---|
| **v0.1** (current target) | **Robinhood Agentic Trading MCP** | An AI decision layer proposes trades for a Robinhood Agentic account, on a schedule or on monitored events. Every proposal clears the risk gate and spend caps, then either waits for your approval or executes within configured limits. Dashboard, scheduler, tamper-evident audit log. |
| **v0.2** (later, same repo) | Solana | Adds on-chain modules — chain client, key custody tiers, wallets, sniper, copy-trade — behind the same `Executor` and feed traits. Additive, not a rewrite. |

The `sniper` and `copytrade` crates exist today as tested library scaffolding. They are **not
wired into the runner** and are deferred to v0.2.

## Why it's split this way

```
          strategies  ──▶  decision layer  ──▶  RiskGate  ──▶  Executor
                                                   ▲              │
                                          the one choke point     ▼
                                                            Portfolio ledger
```

Every strategy funnels through the **same** `RiskGate`. A misbehaving strategy — or an AI
model returning nonsense — can only do as much damage as the gate permits.

## Crates

| Crate | Responsibility | State |
|-------|----------------|-------|
| `sherwood-core` | Domain types, `Portfolio` ledger, `RiskGate`. No I/O. | scaffold, tested |
| `sherwood-execution` | `Executor` trait, `PaperExecutor`, `LiveExecutor` stub | scaffold, tested |
| `sherwood-decision` | `Decider` trait, `RuleDecider`, `AiDecider` (wraps a caller-supplied closure) | scaffold, tested |
| `sherwood-copytrade` | Observed leader trade → sized `Order` | scaffold, **deferred to v0.2** |
| `sherwood-sniper` | `NewPoolEvent` + `RugScreen` → entry `Order` | scaffold, **deferred to v0.2** |
| `sherwood-cli` | `sherwood` binary — paper runner only | scaffold |

Planned crates (not yet written): `store`, `config`, `events`, `supervisor`, `secrets`,
`runtime`, `server`, plus a `frontend/`. See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Run it

```bash
cargo test
cargo run -p sherwood-cli -- demo
```

`demo` runs a synthetic price series through `RuleDecider` → `RiskGate` → `PaperExecutor`
and prints the resulting ledger. It is a wiring demonstration, not a backtest.

```bash
cp config.example.toml config.toml
cargo run -p sherwood-cli -- check config.toml
cargo run -p sherwood-cli -- run config.toml
```

> On Windows, build from **PowerShell**, not Git Bash — Git Bash's coreutils `link` shadows
> the MSVC linker. Requires the MSVC toolchain and VS C++ Build Tools.

## Operator boundary

This project does not, and will not, do these on your behalf:

- Open or authenticate a Robinhood Agentic account, or run `claude mcp add`.
- Accept the Robinhood Crypto customer agreement or the agentic disclosures.
- Hold your credentials, or place a live order without you enabling live mode.

You do those. See [`docs/LIVE_EXECUTION.md`](docs/LIVE_EXECUTION.md).

## Documentation

Start at [`docs/README.md`](docs/README.md) for the full index. Key entry points:

- [`docs/ROADMAP.md`](docs/ROADMAP.md) — phases, steps, MVP definition, non-goals
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — crate map, data flow, event bus
- [`docs/DECISIONS.md`](docs/DECISIONS.md) — ADR index and decision log
- [`docs/SECURITY.md`](docs/SECURITY.md) · [`docs/THREAT-MODEL.md`](docs/THREAT-MODEL.md) · [`docs/AI-SAFETY.md`](docs/AI-SAFETY.md)
- [`docs/ENGINEERING-STANDARDS.md`](docs/ENGINEERING-STANDARDS.md)

## Disclaimer

Experimental software, provided as-is, without warranty. **Not financial, investment, or
trading advice.** Automated strategies — especially ones with a language model in the loop —
can lose the entire account. You are responsible for configuring, supervising, and bearing
the consequences of anything you run.

## License

MIT — but see [`docs/LICENSING.md`](docs/LICENSING.md) before assuming that is final.
