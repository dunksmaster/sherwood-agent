# sherwood-agent

An automated trading system for the Robinhood Agentic Trading MCP, with a hard risk gate in
front of every order.

[![CI](https://github.com/dunksmaster/sherwood-agent/actions/workflows/ci.yml/badge.svg)](https://github.com/dunksmaster/sherwood-agent/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Docs](https://img.shields.io/badge/docs-docs%2F-informational)](docs/README.md)

> **Status: planning + scaffold.** No live venue is wired. The `sherwood` binary refuses any
> mode that is not `paper`, and `LiveExecutor` never fills. See
> [Project status](#project-status) and [`docs/CURRENT-STATE.md`](docs/CURRENT-STATE.md).

## Table of contents

- [Overview](#overview)
- [Scope](#scope)
- [Architecture](#architecture)
- [Repository layout](#repository-layout)
- [Prerequisites](#prerequisites)
- [Getting started](#getting-started)
- [Project status](#project-status)
- [Operator boundary](#operator-boundary)
- [Documentation](#documentation)
- [Contributing](#contributing)
- [Acknowledgements](#acknowledgements)
- [Disclaimer](#disclaimer)
- [License](#license)

## Overview

sherwood-agent proposes trades for a Robinhood Agentic account — on a schedule or in response
to a monitored event — using a decision layer that can be deterministic rules, a language
model, or both. Every proposal clears a risk gate and a set of spend caps before anything
reaches a venue. In manual mode you approve each order; in auto mode it executes within
configured limits. A local dashboard shows the portfolio, an activity feed, pending
approvals, and a kill switch, and every decision, order, and fill is written to a
tamper-evident audit log.

Paper trading is the default. Live requires the venue connected, the admin role, and an
explicit toggle.

## Scope

| Milestone | Venue | What it does |
|---|---|---|
| **v0.1** (current target) | Robinhood Agentic Trading MCP | AI-assisted order automation on a Robinhood Agentic account. OAuth — no wallets, no private keys. Dashboard, scheduler, tamper-evident audit. |
| **v0.2** (later, same repo) | Solana | Adds on-chain modules — chain client, key custody tiers, wallets, sniper, copy-trade — behind the same `Executor` and feed traits. Additive, not a rewrite. |

The `sniper` and `copytrade` crates exist today as tested library scaffolding. They are
**not wired into the runner** and are deferred to v0.2.

## Architecture

Every strategy, every decider, and every venue funnels through the **same** `RiskGate`. A
misbehaving strategy — or a model returning nonsense — can only do as much damage as the gate
permits.

```mermaid
flowchart TB
    F[Feeds<br/>quotes and events] --> D[Decision layer<br/>RuleDecider · AiDecider]
    D -->|Buy / Sell / Hold| SZ[Sizer]
    SZ --> RG{{"RiskGate::check()"}}
    RG -->|reject| DROP[Logged and dropped]
    RG -->|pass| AP{Approval gate}
    AP -->|denied / timeout| DROP
    AP -->|approved / auto| EX[Executor]
    EX --> PAPER[PaperExecutor — default]
    EX --> LIVE[Robinhood — v0.1 live]
    PAPER --> LED[Portfolio ledger]
    LIVE --> LED
    LED -->|position, avg cost| D
```

Full detail, including the event bus and error taxonomy, is in
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Repository layout

```
sherwood-agent/
├── crates/
│   ├── core/         domain types, Portfolio, RiskGate  (no I/O)
│   ├── store/        SQLite persistence + hash-chained audit log (sqlx)
│   ├── execution/    Executor trait, PaperExecutor, LiveExecutor stub
│   ├── decision/     Decider trait, RuleDecider, AiDecider
│   ├── copytrade/    library scaffold — deferred to v0.2
│   ├── sniper/       library scaffold — deferred to v0.2
│   └── cli/          the `sherwood` binary
├── .sqlx/            committed sqlx offline query cache (CI uses this)
├── docs/             the plan, standards, security, ADRs — start at docs/README.md
├── config.example.toml
└── rust-toolchain.toml
```

Planned crates (not yet written): `config`, `events`, `supervisor`, `secrets`,
`runtime`, `server`, plus a `frontend/`. See [`docs/ROADMAP.md`](docs/ROADMAP.md).

## Prerequisites

| # | Requirement | Version | Get it |
|---|---|---|---|
| 1 | Rust toolchain | **1.80+** (stable) | <https://rustup.rs> |
| 2 | C++ build tools (Windows) | VS 2022 Build Tools, "Desktop development with C++" | <https://visualstudio.microsoft.com/downloads/> |
| 3 | C toolchain (Linux) | `build-essential` / `clang` + `pkg-config` | your distro's package manager |

The toolchain is pinned in [`rust-toolchain.toml`](rust-toolchain.toml); `rustup` installs
the right version automatically on first build.

> **Windows:** build from **PowerShell**, not Git Bash. Git Bash's coreutils `link` shadows
> the MSVC linker and produces a confusing "extra operand" error.

## Getting started

```bash
# 1. Clone
git clone https://github.com/dunksmaster/sherwood-agent
cd sherwood-agent

# 2. Test
cargo test

# 3. Run the wiring demo — a synthetic price series through
#    RuleDecider → RiskGate → PaperExecutor, printing the ledger
cargo run -p sherwood-cli -- demo

# 4. Configure
cp config.example.toml config.toml
cargo run -p sherwood-cli -- check config.toml

# 5. Run against the config (paper only)
cargo run -p sherwood-cli -- run config.toml
```

`demo` is a wiring demonstration, not a backtest. There is no real data feed yet — see
[`docs/CURRENT-STATE.md`](docs/CURRENT-STATE.md).

## Project status

**Planning phase.** The scaffold compiles, 23 tests pass, and CI is green, but most of the
system is not built. The plan, the standards, the threat model, and the provenance trail now
live in [`docs/`](docs/README.md) — that was the S0 governance deliverable.

- **Done:** the S0 doc set, a tested core with `RiskGate` and `Portfolio`, a deterministic
  paper executor, a rule-based decider.
- **Next:** accept [ADR-0001](docs/adr/0001-mcp-interaction-model.md) — how sherwood reaches
  the Robinhood MCP. Everything in Phase 3 is downstream of that one choice.
- **Not started:** persistence, event bus, real data feed, the Robinhood adapter, the server,
  the dashboard. See the roadmap.

Roadmap and step list: [`docs/ROADMAP.md`](docs/ROADMAP.md). Known defects in the current
code: [`docs/CURRENT-STATE.md`](docs/CURRENT-STATE.md).

Once a dashboard exists, this README will grow a screenshot-driven walkthrough of each
feature. It does not have one yet because there is nothing to show.

## Operator boundary

This project does not, and will not, do these on your behalf:

- Open or authenticate a Robinhood Agentic account, or run `claude mcp add`.
- Accept the Robinhood Crypto customer agreement or the agentic disclosures.
- Hold your credentials, or place a live order without you enabling live mode.

You do those. See [`docs/LIVE_EXECUTION.md`](docs/LIVE_EXECUTION.md).

## Documentation

Start at [`docs/README.md`](docs/README.md) for the indexed set. Entry points:

| Document | What it covers |
|---|---|
| [ROADMAP.md](docs/ROADMAP.md) | Phases, steps S0–S16, MVP definition, non-goals, v0.2 outline |
| [ARCHITECTURE.md](docs/ARCHITECTURE.md) | Crate map, data flow, event bus, error taxonomy |
| [DECISIONS.md](docs/DECISIONS.md) | ADR index and the decision log |
| [ENGINEERING-STANDARDS.md](docs/ENGINEERING-STANDARDS.md) | Lints, MSRV, determinism, testing, CI gates |
| [SECURITY.md](docs/SECURITY.md) · [THREAT-MODEL.md](docs/THREAT-MODEL.md) · [AI-SAFETY.md](docs/AI-SAFETY.md) | Security architecture, STRIDE, prompt-injection defence |
| [LICENSING.md](docs/LICENSING.md) · [PRIOR-ART.md](docs/PRIOR-ART.md) | Clean-room policy and provenance trail |

## Contributing

Contributions are welcome; the bar is high because this trades a funded account. Read
[`CONTRIBUTING.md`](CONTRIBUTING.md) first — it carries the review checklist and a
**Contributor Licence Agreement** that every PR accepts by submission.

## Acknowledgements

Design concepts were studied from several projects — **OpenTrade-Agent** (the Robinhood
harness pattern), **barter-rs** and **NautilusTrader** (event-driven trait design),
**freqtrade** (strategy and protection patterns), among others. No source code from any
non-permissively-licensed project is used here. The full list, with licences and a
"code used" column, is [`docs/PRIOR-ART.md`](docs/PRIOR-ART.md).

## Disclaimer

Experimental software, provided as-is, without warranty. **Not financial, investment, or
trading advice.** Automated strategies — especially ones with a language model in the loop —
can lose the entire account. You are responsible for configuring, supervising, and bearing
the consequences of anything you run.

## License

[MIT](LICENSE) — but see [`docs/LICENSING.md`](docs/LICENSING.md) before assuming that is
final.
