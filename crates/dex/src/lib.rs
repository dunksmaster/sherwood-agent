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
//! struct field order), then diffed byte-for-byte against a real,
//! currently-successful `execute` transaction pulled from the live chain —
//! every field matched exactly. That confirms the **encoding** is right. It
//! does **not** confirm a swap against a real pool will succeed: simulating
//! (`sherwood dex-simulate`) a swap on the live NVDA/USDG pool still
//! reverts, from a wallet with confirmed balance and maxed-out approvals.
//! Narrowed but not solved: it isn't the RPC plumbing, isn't "wrong pool by
//! liquidity ranking" (tried two pools at very different fee tiers, both
//! revert identically), and isn't the Permit2/SETTLE or PoolManager/TAKE
//! steps (both proven to work in isolation, directly against the live
//! chain) — leaving the swap action itself as the remaining suspect, by
//! elimination. See `crates/dex/README.md`. **Do not sign or broadcast
//! anything this crate builds until `sherwood dex-simulate` for that exact
//! pool returns success.**

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
