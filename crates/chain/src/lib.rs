//! Read-only EVM client for Robinhood Chain ([ADR-0006](../../../docs/adr/0006-robinhood-chain-venue.md)).
//!
//! Scope of this crate: connect to an EVM JSON-RPC endpoint and *read*. It
//! exposes exactly the calls sherwood needs to
//!
//! * identify the chain ([`EvmClient::chain_id`], [`EvmClient::block_number`]),
//! * read an ERC-20 ([`erc20::Erc20`]) — metadata and balances, and
//! * check that a token's transfers are permissionless
//!   ([`probe::check_transfer_open`]) — the mandatory pre-flight before live
//!   mode may arm.
//!
//! It **never** builds, signs, or broadcasts a transaction. There is no method
//! that takes a private key or returns a signed payload. Transaction
//! construction and signing are `sherwood-signer` / `sherwood-dex` (v0.2.2+),
//! behind their own gates.
//!
//! The transport is behind the [`EvmClient`] trait so tests drive the ABI and
//! decode paths against canned JSON-RPC responses; [`HttpClient`] is the real
//! `reqwest`-backed implementation.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod abi;
pub mod erc20;
pub mod keccak;
pub mod probe;
pub mod rpc;

#[cfg(test)]
mod testutil;

pub use rpc::{EvmClient, HttpClient, LogFilter, RpcLog};

/// Anything that can go wrong reading the chain.
#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    /// The HTTP request itself failed (DNS, TLS, connection, non-2xx).
    #[error("rpc transport: {0}")]
    Transport(String),
    /// A JSON-RPC error object came back (`{ "error": { code, message } }`).
    #[error("rpc error {code}: {message}")]
    Rpc { code: i64, message: String },
    /// The response was 2xx JSON but not the shape we expected.
    #[error("rpc response: {0}")]
    Decode(String),
    /// A contract call reverted. `data` is the ABI-encoded revert payload when
    /// the node returned one (custom-error selector + args, or `Error(string)`).
    #[error("call reverted{}", .reason.as_deref().map(|r| format!(": {r}")).unwrap_or_default())]
    Reverted {
        reason: Option<String>,
        data: Option<String>,
    },
    /// We asked for something the chain does not have (no code at an address,
    /// an empty result where a value was required, …).
    #[error("{0}")]
    NotFound(String),
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, ChainError>;
