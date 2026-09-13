//! Shared "build a `V4_SWAP`, then `eth_call`-simulate it" logic behind both
//! `sherwood dex-simulate` ([`dex_simulate_cmd`](crate::dex_simulate_cmd)) and
//! `POST /v1/dex/simulate` (v0.2.9). One read (`eth_call`); never a send —
//! same boundary as `sherwood-dex` itself.

use anyhow::{Context, Result};
use rust_decimal::Decimal;
use sherwood_chain::tokens::{self, POOL_MANAGER, STATE_VIEW};
use sherwood_chain::univ4::{self, Decimals};
use sherwood_chain::{EvmClient, HttpClient};
use sherwood_dex::{quote, ExactInputSingleSwap};
use sherwood_server::state::{DexSimulateOutcome, DexSimulateRequest, DexSimulator};
use std::time::Duration;

pub const UNIVERSAL_ROUTER: &str = "0x8876789976decbfcbbbe364623c63652db8c0904";

/// A built swap, ready to `eth_call`-simulate — everything `dex-simulate`
/// prints before it actually calls the chain, minus the printing.
pub struct Preview {
    pub head: u64,
    pub token_symbol: String,
    pub denom_symbol: String,
    pub pool_fee: u32,
    pub pool_tick_spacing: i32,
    pub pool_liquidity: u128,
    pub amount_in: u128,
    pub amount_out_minimum: u128,
    pub slippage_bps: u32,
    pub calldata: Vec<u8>,
}

/// Resolve `token_arg`/`denom_arg`, find the best pool, and build the
/// `execute` calldata for an exact-input swap. Does not call the chain with
/// `from` — that's [`simulate`].
pub async fn build<C: EvmClient>(
    client: &C,
    token_arg: &str,
    amount_in: u128,
    denom_arg: &str,
    slippage_bps: u32,
) -> Result<Preview> {
    let (token_symbol, token_addr, token_dec) = tokens::resolve(token_arg);
    let (denom_symbol, denom_addr, denom_dec) = tokens::resolve(denom_arg);

    let head = client
        .block_number()
        .await
        .context("connecting to the RPC")?;

    let best = univ4::find_best_pool(
        client,
        univ4::Deployment {
            pool_manager: POOL_MANAGER,
            state_view: STATE_VIEW,
        },
        &token_addr,
        &denom_addr,
        0,
        head,
    )
    .await
    .with_context(|| format!("finding a {token_symbol}/{denom_symbol} pool"))?;

    let spot_price = univ4::quote_pool(
        client,
        STATE_VIEW,
        &best.key,
        &token_addr,
        Decimals {
            token: token_dec,
            denominator: denom_dec,
        },
    )
    .await
    .context("reading the spot price")?;

    let zero_for_one = best.key.currency0.eq_ignore_ascii_case(&token_addr);
    // Rough expected output from the spot price (already denom-per-token,
    // regardless of which side is currency0/1) — a real caller should use a
    // live quoter for anything that matters; this is a simulation aid.
    let scale = 10u128.pow(u32::from(denom_dec));
    let expected_out_human =
        Decimal::from(amount_in) / Decimal::from(10u128.pow(u32::from(token_dec))) * spot_price;
    let expected_out_raw: u128 = (expected_out_human * Decimal::from(scale))
        .try_into()
        .context("expected output does not fit u128")?;
    let amount_out_minimum = quote::amount_out_minimum(expected_out_raw, slippage_bps)?;

    let swap = ExactInputSingleSwap {
        pool: best.key,
        zero_for_one,
        amount_in,
        amount_out_minimum,
    };
    let deadline = quote::deadline_from_now(1800);
    let calldata = swap.execute_calldata(deadline)?;

    Ok(Preview {
        head,
        token_symbol,
        denom_symbol,
        pool_fee: swap.pool.fee,
        pool_tick_spacing: swap.pool.tick_spacing,
        pool_liquidity: best.liquidity,
        amount_in,
        amount_out_minimum,
        slippage_bps,
        calldata,
    })
}

/// `eth_call` the built swap as `from`. `Ok` means the calldata is
/// well-formed and `from` has the balance + Permit2 allowance it needs;
/// `Err` could be either a real encoding problem or simply that `from`
/// lacks balance/allowance — both look like a revert here.
pub async fn simulate<C: EvmClient>(
    client: &C,
    from: &str,
    preview: &Preview,
) -> sherwood_chain::Result<Vec<u8>> {
    client
        .call_from(
            from,
            UNIVERSAL_ROUTER,
            &sherwood_chain::abi::to_hex(&preview.calldata),
        )
        .await
}

/// Wires [`build`] + [`simulate`] into `sherwood_server::state::DexSimulator`
/// for `sherwood serve` (`POST /v1/dex/simulate`). Same boundary as the CLI
/// command: one `eth_call`, nothing signed, nothing sent.
pub struct ChainDexSimulator {
    pub rpc_url: String,
}

#[async_trait::async_trait]
impl DexSimulator for ChainDexSimulator {
    async fn simulate(&self, req: DexSimulateRequest) -> Result<DexSimulateOutcome, String> {
        let amount_in: u128 = req
            .amount_in_raw
            .parse()
            .map_err(|e| format!("amount_in_raw must be an integer (base units): {e}"))?;
        let slippage_bps = req.slippage_bps.unwrap_or(50);
        let denom = req.denom.as_deref().unwrap_or("USDG");

        let client = HttpClient::new(self.rpc_url.clone(), Duration::from_secs(30))
            .map_err(|e| format!("connecting to {}: {e}", self.rpc_url))?;

        let preview = build(&client, &req.token, amount_in, denom, slippage_bps)
            .await
            .map_err(|e| format!("{e:#}"))?;

        let (ok, detail) = match simulate(&client, &req.from, &preview).await {
            Ok(ret) => (
                true,
                format!("eth_call succeeded — {} bytes returned", ret.len()),
            ),
            Err(e) => (false, format!("eth_call reverted: {e}")),
        };

        Ok(DexSimulateOutcome {
            token_symbol: preview.token_symbol,
            denom_symbol: preview.denom_symbol,
            pool_fee: preview.pool_fee,
            pool_tick_spacing: preview.pool_tick_spacing,
            pool_liquidity: preview.pool_liquidity.to_string(),
            amount_out_minimum: preview.amount_out_minimum.to_string(),
            calldata_hex: sherwood_chain::abi::to_hex(&preview.calldata),
            ok,
            detail,
        })
    }
}
