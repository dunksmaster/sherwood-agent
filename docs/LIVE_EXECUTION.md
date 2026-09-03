# Going live

sherwood-agent ships without a live execution adapter **on purpose**. Wiring one
up is your responsibility, and it comes with obligations this project cannot
discharge for you:

- You open and authenticate any brokerage / exchange account yourself.
- You accept that venue's customer agreement and disclosures yourself.
- You hold the keys / tokens. Nothing in this repo should ever contain a secret;
  `config.toml`, `.env`, and `*.key` are gitignored.
- You are responsible for every order a live adapter places. An automated
  strategy — especially one with an LLM in the loop — can lose the whole
  account. Test on paper first, size small, keep the kill switch reachable.

## What you implement

One trait:

```rust
#[async_trait]
impl sherwood_execution::Executor for MyVenue {
    async fn execute(&self, order: &sherwood_core::Order)
        -> Result<sherwood_core::Fill, sherwood_execution::ExecError> { /* ... */ }

    fn name(&self) -> &'static str { "myvenue" }
}
```

Then build your own binary that constructs `MyVenue` instead of `PaperExecutor`
and calls the same runner flow (`RiskGate::check` → `Executor::execute` →
`Portfolio::apply`). Do not modify `sherwood-cli` to do this — keep the shipped
binary paper-only so it stays safe to run.

## Robinhood Agentic Trading MCP

Robinhood exposes an MCP server that can place orders in a dedicated "Agentic"
account. If you choose to use it:

1. Follow Robinhood's own setup (desktop only): connect the MCP to your agent
   platform, open the Agentic account, accept the updated Crypto customer
   agreement and agentic disclosures. **You do this, in Robinhood's UI.**
2. Write an `Executor` impl that calls the MCP's order tools with the fields
   from `Order`. Map `ExecError` onto its failure modes.
3. Keep `max_order_notional`, `max_position_fraction`, and `max_daily_loss` in
   the risk config tight. The gate is your backstop against a bad decision.

This repo will not add that adapter or run that setup for you.
