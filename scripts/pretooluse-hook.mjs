#!/usr/bin/env node
// PreToolUse hook — the bridge for ADR-0001 Option 3.
//
// A headless `claude` / `codex` agent holding the Robinhood MCP runs this
// before every tool call. It forwards the call to `sherwood-server`
// (`POST /v1/hook/pretooluse`), which runs the risk gate + approval gate +
// session budget, and maps the `{decision}` answer onto the agent CLI's
// permission schema. It fails closed: any error, timeout, or non-allow answer
// blocks the tool call.
//
// Wiring (Claude Code, .claude/settings.json):
//   { "hooks": { "PreToolUse": [ { "matcher": "*", "hooks": [
//       { "type": "command", "command": "node /abs/path/scripts/pretooluse-hook.mjs" }
//   ] } ] } }
//
// Environment:
//   SHERWOOD_API_URL     default http://127.0.0.1:8787
//   SHERWOOD_API_TOKEN    required — an operator-role token (`sherwood secrets get api_token`)
//   SHERWOOD_HOOK_CONTEXT path to a JSON file with the account/market context
//                         the server builds a GateContext from:
//                           { "portfolio": <serialised core::Portfolio>,
//                             "equity": "…", "unrealized_pnl": "…",
//                             "ref_price": "…"?, "last_order_at": "…"? }
//                         Effectively required for buys: absent, a flat zero-cash
//                         portfolio is sent and the gate denies every buy for
//                         insufficient cash. Reads and cancels still pass, and
//                         the config-level checks (kill switch, allow/deny list,
//                         notional/slippage caps, session budget, approval gate)
//                         always apply.
//   SHERWOOD_HOOK_TIMEOUT_MS  default 90000 (must exceed the server's approval timeout)
//   SHERWOOD_HOOK_DEBUG   set to print the request/response to stderr
//
// `--dry-run`: read a hook event on stdin, print the request that would be sent,
// exit 0. For testing against a local `sherwood serve` without an agent.

import { readFileSync } from "node:fs";

const URL_BASE = (process.env.SHERWOOD_API_URL || "http://127.0.0.1:8787").replace(/\/$/, "");
const TOKEN = process.env.SHERWOOD_API_TOKEN;
const TIMEOUT_MS = Number(process.env.SHERWOOD_HOOK_TIMEOUT_MS || 90_000);
const DEBUG = !!process.env.SHERWOOD_HOOK_DEBUG;
const DRY_RUN = process.argv.includes("--dry-run");

function dbg(...a) {
  if (DEBUG || DRY_RUN) console.error("[pretooluse-hook]", ...a);
}

/** Deny in whatever schema the agent CLI expects, then exit. */
function deny(reason) {
  // Claude Code's structured form:
  process.stdout.write(
    JSON.stringify({
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: "deny",
        permissionDecisionReason: reason,
      },
    }),
  );
  // Belt-and-braces for hook runners that key off exit code + stderr:
  console.error(reason);
  process.exit(2);
}

function allow() {
  process.stdout.write(
    JSON.stringify({
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: "allow",
        permissionDecisionReason: "sherwood: allowed",
      },
    }),
  );
  process.exit(0);
}

function loadContext() {
  const p = process.env.SHERWOOD_HOOK_CONTEXT;
  if (p) {
    try {
      return JSON.parse(readFileSync(p, "utf8"));
    } catch (e) {
      deny(`sherwood: cannot read SHERWOOD_HOOK_CONTEXT (${p}): ${e.message}`);
    }
  }
  // Flat, empty account. `ref_price` is left for the server to fall back to the
  // order's own limit price.
  return {
    portfolio: { cash: "0", positions: {}, realized_pnl: "0", avg_cost: {} },
    equity: "0",
    unrealized_pnl: "0",
  };
}

async function readStdin() {
  const chunks = [];
  for await (const c of process.stdin) chunks.push(c);
  return Buffer.concat(chunks).toString("utf8");
}

const raw = await readStdin();
let event;
try {
  event = JSON.parse(raw);
} catch (e) {
  deny(`sherwood: hook stdin is not JSON: ${e.message}`);
}

const toolName = event.tool_name ?? event.toolName ?? "";
const toolInput = event.tool_input ?? event.toolInput ?? {};
if (!toolName) deny("sherwood: hook event has no tool_name");

const body = { tool_call: { name: toolName, arguments: toolInput }, context: loadContext() };

if (DRY_RUN) {
  console.log(JSON.stringify({ url: `${URL_BASE}/v1/hook/pretooluse`, body }, null, 2));
  process.exit(0);
}

if (!TOKEN) deny("sherwood: SHERWOOD_API_TOKEN is not set — cannot reach the gate");

const ctrl = new AbortController();
const timer = setTimeout(() => ctrl.abort(), TIMEOUT_MS);
let resp, text;
try {
  resp = await fetch(`${URL_BASE}/v1/hook/pretooluse`, {
    method: "POST",
    headers: { "content-type": "application/json", authorization: `Bearer ${TOKEN}` },
    body: JSON.stringify(body),
    signal: ctrl.signal,
  });
  text = await resp.text();
} catch (e) {
  deny(`sherwood: gate unreachable (${e.name === "AbortError" ? "timeout" : e.message})`);
} finally {
  clearTimeout(timer);
}

dbg("HTTP", resp.status, text);

if (!resp.ok) {
  let msg = text;
  try {
    msg = JSON.parse(text).message || text;
  } catch {}
  deny(`sherwood: gate returned ${resp.status}: ${msg}`);
}

let out;
try {
  out = JSON.parse(text);
} catch (e) {
  deny(`sherwood: gate response is not JSON: ${e.message}`);
}

if (out.decision === "allow") allow();
deny(`sherwood: ${out.reason || "denied"}`);
