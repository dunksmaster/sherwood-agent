# scripts/

| File | What |
|---|---|
| `check-doc-links.py` | CI: verify every relative link in the Markdown docs resolves. |
| `pretooluse-hook.mjs` | The `PreToolUse` hook for [ADR-0001](../docs/adr/0001-mcp-interaction-model.md) Option 3 — forwards an agent's tool call to `sherwood-server` and maps the allow/deny answer onto the agent CLI's permission schema. Fails closed. |

## `pretooluse-hook.mjs`

Node ≥ 18 (uses `fetch`). No dependencies.

### Wire it up (Claude Code)

`.claude/settings.json` in the directory the trading agent runs from:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "*",
        "hooks": [
          { "type": "command", "command": "node /abs/path/to/scripts/pretooluse-hook.mjs" }
        ]
      }
    ]
  }
}
```

### Environment

| Var | Default | |
|---|---|---|
| `SHERWOOD_API_TOKEN` | — (**required**) | an `operator`-role token — `sherwood secrets get api_token` |
| `SHERWOOD_API_URL` | `http://127.0.0.1:8787` | where `sherwood serve` is listening |
| `SHERWOOD_HOOK_CONTEXT` | — | path to a JSON file with `{ portfolio, equity, unrealized_pnl, ref_price?, last_order_at? }` — the account/market picture the gate's `GateContext` is built from. **Effectively required for buys:** absent, the hook sends a flat zero-cash portfolio and the risk gate denies every buy for insufficient cash (reads and cancels still pass; the config-level checks — kill switch, allow/deny list, notional/slippage caps, session budget, approval gate — always apply). Point it at a file with real balances once you have live account data. |
| `SHERWOOD_HOOK_TIMEOUT_MS` | `90000` | must exceed `[server] approval_timeout_secs` × 1000, or a slow-but-valid approval becomes a transport error |
| `SHERWOOD_HOOK_DEBUG` | — | print the HTTP request/response to stderr |

### Test it without an agent

```bash
# 1. start the API
SHERWOOD_VAULT_PASSPHRASE=… cargo run -p sherwood-cli -- serve config.toml

# 2. see the request the hook would send
echo '{"hook_event_name":"PreToolUse","tool_name":"place_order",
       "tool_input":{"symbol":"ROAR","side":"buy","quantity":"1","limit_price":"100"}}' \
  | node scripts/pretooluse-hook.mjs --dry-run

# 3. run it for real against the gate
export SHERWOOD_API_TOKEN=$(cargo run -q -p sherwood-cli -- secrets get api_token)
echo '{"hook_event_name":"PreToolUse","tool_name":"place_order",
       "tool_input":{"symbol":"ROAR","side":"buy","quantity":"1","limit_price":"100"}}' \
  | SHERWOOD_HOOK_DEBUG=1 node scripts/pretooluse-hook.mjs ; echo "exit $?"
```

The config's `[hook]` section must list `place_order` under `place_tools` for a
place order to be allowed.

### Status

The script → server → risk/approval/budget gate → mapped-decision chain is
**tested end to end** against a local `sherwood serve` (allow, over-notional
deny, unlisted-tool deny, insufficient-cash deny, read-tool allow). What has
**not** been exercised is a real headless `claude` / `codex` agent invoking it
with a live Robinhood MCP attached — that is the first task of S7 once a
connection exists. The Codex hook schema differs from Claude Code's; adapt
`allow()` / `deny()` accordingly.
