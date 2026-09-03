//! Value types. All prices and quantities are [`Decimal`] to avoid float drift.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A tradable asset, identified by a venue-agnostic symbol plus an optional
/// on-chain address (used on Robinhood Chain / EVM venues).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Asset {
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
}

impl Asset {
    pub fn symbol(s: impl Into<String>) -> Self {
        Self { symbol: s.into(), address: None }
    }

    pub fn onchain(symbol: impl Into<String>, address: impl Into<String>) -> Self {
        Self { symbol: symbol.into(), address: Some(address.into()) }
    }
}

impl fmt::Display for Asset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.symbol)
    }
}

/// Where an order is routed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Venue {
    /// Simulated venue — never touches a network.
    Paper,
    /// Robinhood Agentic Trading MCP. Adapter is user-supplied; see the
    /// `execution` crate.
    RobinhoodMcp,
    /// A generic on-chain DEX router (e.g. a Uniswap-style contract).
    DexRouter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Buy,
    Sell,
}

/// Opaque, client-generated order identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrderId(pub String);

impl OrderId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl fmt::Display for OrderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An intent to trade, produced by a strategy and not yet risk-checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Order {
    pub id: OrderId,
    pub asset: Asset,
    pub side: Side,
    /// Quantity in base-asset units.
    pub qty: Decimal,
    /// Limit price. `None` means "market" — the executor decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_price: Option<Decimal>,
    /// Maximum tolerated slippage as a fraction (0.01 == 1%).
    pub max_slippage: Decimal,
    pub venue: Venue,
    /// Free-form provenance, e.g. "copytrade:0xabc" or "sniper:new_pool".
    pub reason: String,
}

/// The result of an executor acting on an [`Order`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fill {
    pub order_id: OrderId,
    pub asset: Asset,
    pub side: Side,
    pub qty: Decimal,
    pub price: Decimal,
    /// Fee paid, quoted in the portfolio's cash asset.
    pub fee: Decimal,
    pub venue: Venue,
    pub at: DateTime<Utc>,
}

impl Fill {
    /// Signed cash delta this fill applies to a portfolio's cash balance
    /// (negative for buys, positive for sells), fee included.
    pub fn cash_delta(&self) -> Decimal {
        let gross = self.qty * self.price;
        match self.side {
            Side::Buy => -(gross + self.fee),
            Side::Sell => gross - self.fee,
        }
    }
}

/// A point-in-time view of an asset's market, fed to the decision layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketSnapshot {
    pub asset: Asset,
    pub price: Decimal,
    /// 24h price change as a fraction.
    pub change_24h: Decimal,
    /// Pool / book liquidity in the cash asset, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub liquidity: Option<Decimal>,
    pub at: DateTime<Utc>,
}

/// A raw observation a strategy emits before it becomes an [`Order`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    pub asset: Asset,
    pub kind: SignalKind,
    /// Strategy confidence in `[0, 1]`.
    pub confidence: Decimal,
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    Enter,
    Exit,
    Scale,
}

/// The decision layer's verdict on a [`MarketSnapshot`] (+ context).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Decision {
    /// Open or add to a position sized as a fraction of equity.
    Buy { fraction: Decimal, reason: String },
    /// Close or trim a position by a fraction of the current holding.
    Sell { fraction: Decimal, reason: String },
    Hold { reason: String },
}
