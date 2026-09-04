---
status: partial
last-updated: 2026-09-04
owner-step: S14
---

# Backtest

```
sherwood backtest config.toml
```

Replays the `[general] feed_path` CSV (`timestamp,symbol,price`) through the
configured decider and the risk gate — the **same** `run_loop` as `sherwood
run`, same paper executor — and prints a performance summary. Nothing is
persisted.

## Metrics

| Line | Meaning |
|---|---|
| total return | `(final_equity − starting_cash) / starting_cash` |
| max drawdown | largest peak-to-trough drop of the per-tick equity curve |
| fills | number of executed orders |
| closed trades | sells that reduced or flattened a position (a partial sell closes a proportional slice, average-cost basis) |
| win rate | winning closed trades / closed trades |
| gross profit / loss | sums of positive / negative closed-trade P&L |
| profit factor | `gross_profit / |gross_loss|` (n/a with no losing trades) |
| expectancy / trade | mean closed-trade P&L |

## What it does *not* tell you

- **Only the configured decider is exercised.** With `decider = "ai"` the
  backtest makes a real provider call per tick — slow and metered. Side-by-side
  A/B of two deciders is a later addition.
- **No slippage/impact model beyond the paper executor's** fixed spread + fee.
- **`order_cooldown_secs` is forced to `0`.** A backtest replays in
  microseconds, so a wall-clock cooldown would block every order after the
  first. Cooldown is a live-trading control; it is exercised in `sherwood run`.
- **Per [ADR-0001](adr/0001-mcp-interaction-model.md) the live decision path is
  the external agent + hook**, which this harness cannot replay. These numbers
  describe `RuleDecider` / the closure-or-provider `AiDecider` against recorded
  prices — not live agentic performance.
- No transaction-cost sensitivity, no parameter sweep, no walk-forward.

Treat the output as a sanity check on strategy logic, not a forecast.
