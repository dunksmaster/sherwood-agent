---
status: stub
last-updated: 2026-09-03
owner-step: S7
---

# Robinhood integration facts

**Not yet written.** Filled at **S7**, and partially researchable earlier — several of these
answers shape the `Portfolio` model and must land before S5 sizing work is final.

## Known from public documentation

- Endpoint: `https://agent.robinhood.com/mcp/trading`, Streamable HTTP transport,
  OAuth-authenticated, consent completed in a desktop browser.
- Requires an existing individual investing account in good standing; the **Agentic** account
  is opened during the MCP connection flow.
- Read scope: accounts and account numbers, positions, balances, transaction and order
  history, watchlists and scans.
- Write scope: place and cancel orders **in the Agentic account only**.
- Crypto requires a Robinhood Crypto account and acceptance of the updated customer agreement
  plus agentic disclosures. Agents may **not** transfer, stake, or lend crypto.
- Agentic crypto trading is unavailable in some states, including New York.

## Must be established before S7

| Question | Why it matters |
|---|---|
| Exact MCP tool names and argument schemas | The S7.2 tool allowlist and the approval-gate parser depend on them |
| Which asset classes are reachable — equities, crypto, options? | Each has different settlement, order types, and risk |
| Supported order types — market, limit, stop, stop-limit, trailing? | Determines what `Order` can express |
| Fractional versus whole-share sizing, and per-symbol availability | Changes sizing arithmetic |
| Settlement — T+1 for equities, T+0 for crypto | **Affects cash-available-for-trading in `Portfolio`**; ignoring it overstates buying power |
| Pattern Day Trader rule interaction | More than three day trades in five days under \$25k locks the account |
| Rate limits — per minute, per hour | Shapes scheduler cadence and the quota manager |
| Error codes and messages | Feeds the mapping to `Transient` / `Fatal` / `Rejected` in the [error taxonomy](ARCHITECTURE.md#error-taxonomy) |
| Order-ledger pagination and latency | Drives the reconciliation loop in S7.4 |
| Whether OAuth consent authorises a non-listed MCP client | **Decides whether Option 1 in [ADR-0001](adr/0001-mcp-interaction-model.md) is viable at all** |

Facts go here with a citation. Nothing in this file should be inferred.
