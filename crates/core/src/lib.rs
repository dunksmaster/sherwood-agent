//! Core domain types shared by every sherwood-agent crate.
//!
//! This crate is deliberately free of I/O. It defines the vocabulary the rest
//! of the system speaks: assets, orders, fills, portfolio state, and the risk
//! gate that every order must pass before an executor is allowed to see it.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
pub mod clock;
pub mod portfolio;
pub mod risk;
pub mod types;

pub use clock::{Clock, FixedClock, SystemClock};
pub use portfolio::Portfolio;
pub use risk::{GateContext, RiskConfig, RiskGate, RiskReject};
pub use types::{
    Asset, Decision, Fill, MarketSnapshot, Order, OrderId, Side, Signal, SignalKind, Venue,
};
