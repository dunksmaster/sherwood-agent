---
status: accepted
last-updated: 2026-09-03
owner-step: S0
---

# Going live

This repository ships without a working live execution path, on purpose. The `sherwood`
binary refuses any mode that is not `paper`, and `LiveExecutor` returns
`ExecError::LiveNotConfigured` on every call.

## The operator boundary

These steps are yours. They are not automated here, and they will not be:

1. **Open and authenticate a Robinhood Agentic account.** Desktop browser only. The account
   is created during the MCP connection flow; you must already have an individual investing
   account in good standing.
2. **Run the MCP connection yourself** —
   `claude mcp add robinhood-trading --transport http https://agent.robinhood.com/mcp/trading`,
   then `/mcp` to authenticate.
3. **Read and accept** the Robinhood Crypto customer agreement and the agentic disclosures,
   if you intend to trade crypto.
4. **Enable live mode deliberately** — admin role, explicit toggle, re-authentication. The
   first live order requires manual approval regardless of your configured approval mode.

You are responsible for every order the system places. An automated strategy with a language
model in the loop can lose the entire account. Size small, keep the kill switch reachable,
and run in paper long enough to know what the thing actually does.

## What has to exist first

Live execution is **S7–S8** on the [roadmap](ROADMAP.md), and it is gated on
[ADR-0001](adr/0001-mcp-interaction-model.md) being accepted. Nothing before that is
sufficient:

| Prerequisite | Step |
|---|---|
| Durable state and a verifiable audit chain | S1 |
| Validated configuration | S2 |
| Event bus and supervisor | S3 |
| An end-to-end paper loop with the extended risk gate and a working kill switch | S5 |
| Credential vault | S6 |
| MCP client or agent harness, tool allowlist, order reconciliation | S7 |
| Session lifecycle that fails closed | S8 |
| Approval gate | S11 |

## The seam

Whatever the venue, live execution enters through one trait:

```rust
#[async_trait]
impl sherwood_execution::Executor for MyVenue {
    async fn execute(&self, order: &sherwood_core::Order)
        -> Result<sherwood_core::Fill, sherwood_execution::ExecError> { /* ... */ }

    fn name(&self) -> &'static str { "myvenue" }
}
```

Two rules hold regardless of implementation:

- **Nothing reaches an executor without passing `RiskGate::check` first.** This is the
  project's central invariant.
- **Keep the shipped `sherwood` CLI paper-only.** A live venue is wired in your own binary or
  behind an explicit, off-by-default feature — never by loosening the mode guard.

## Under ADR-0001 Option 3

The current recommendation is an agent harness: a headless `claude` or `codex` process holds
the MCP connection, and every venue tool call is intercepted by a **fail-closed `PreToolUse`
hook** that calls back into `sherwood-server` for the risk gate and the approval gate.

In that model the hook *is* the safety boundary. Three properties are non-negotiable:

1. **Fails closed.** No response, a timeout, or a malformed reply means the order is denied.
2. **Tool allowlist.** Unrecognised tool names are denied, never passed through.
3. **Timeout ordering.** The hook's timeout must exceed the approval timeout, and the agent's
   hook timeout must exceed the hook's — otherwise a slow human approval is silently
   converted into a denial.

## Keys

v0.1 holds none. Robinhood authenticates over OAuth and custodies the assets, and under
Option 3 that grant lives inside the CLI agent's own credential store, never in this
codebase.

Key custody — encrypted keystores, hardware wallets, HSM tiers, signer isolation — arrives
with Solana in v0.2. When it does, the rule is unchanged from day one: **the application
never accepts a private key through a form and never persists one.**
