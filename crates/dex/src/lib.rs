//! Uniswap v4 swap construction on Robinhood Chain
//! ([ADR-0006](../../../docs/adr/0006-robinhood-chain-venue.md), v0.2.4).
//!
//! This crate builds calldata: a single-hop exact-input `V4_SWAP` through
//! the `UniversalRouter`, plus the two Permit2 approvals it depends on. It
//! can, via `sherwood-signer`, sign the transaction that calldata belongs
//! to. **It has no RPC client and no method that broadcasts anything** —
//! same boundary as every crate below it in the v0.2 stack.
//!
//! Every byte layout in [`v4swap`] was checked against `Uniswap/v4-periphery`
//! and `Uniswap/universal-router`'s own source (command/action constants,
//! struct field order), then diffed word-by-word against a real,
//! currently-successful `execute` transaction pulled from the live chain.
//! That diff caught the one bug that made every simulated swap revert with
//! empty data: the `ExactInputSingleParams` `hookData` offset was encoded as
//! `9*32` (the pre-`minHopPriceX36` layout) instead of `10*32`, so
//! `CalldataDecoder` sliced out of bounds inside `PoolManager.unlock`. Fixed
//! in `v4swap.rs`; a local Foundry fork replay (`debug/foundry/`) shows the
//! pre-fix bytes reverting empty and the post-fix bytes simulating clean
//! against the live NVDA/USDG pool. `sherwood dex-simulate` for that pool
//! now returns success from a funded, Permit2-approved wallet.
//!
//! **Still: `sherwood dex-simulate` for the exact pool you intend to trade
//! is the gate. Do not sign or broadcast anything this crate builds until it
//! returns success for that pool, from your actual wallet.**

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod abi_dyn;
pub mod permit2;
pub mod quote;
pub mod v4swap;

pub use v4swap::ExactInputSingleSwap;

/// Anything that can go wrong building a swap.
#[derive(Debug, thiserror::Error)]
pub enum DexError {
    #[error("slippage_bps must be < 10_000, got {0}")]
    InvalidSlippage(u32),
    #[error("amount overflow")]
    Overflow,
    #[error(transparent)]
    Chain(#[from] sherwood_chain::ChainError),
}
