//! Turn an agent's order-tool arguments into a [`core::Order`] the risk gate
//! can check. Used by the `PreToolUse` [`hook`](crate::hook).
//!
//! The mapping is deliberately strict. The Robinhood MCP's exact argument
//! schema is an open item on ADR-0001; until it is pinned down this accepts the
//! common shape — `symbol`, `side`, and either an explicit `quantity` or a
//! `notional` amount plus a price to size it — and rejects anything it cannot
//! represent exactly. A rejection here becomes a hook *denial*, never a
//! pass-through, so "strict" is the safe direction to err.

use rust_decimal::Decimal;
use serde_json::Value;
use sherwood_core::{Asset, Order, OrderId, Side, Venue};
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OrderParseError {
    #[error("arguments are not a JSON object")]
    NotAnObject,
    #[error("missing required field `{0}`")]
    MissingField(&'static str),
    #[error("field `{field}` is invalid: {detail}")]
    BadField { field: &'static str, detail: String },
    #[error("side `{0}` is not `buy` or `sell`")]
    UnknownSide(String),
    #[error("cannot size the order: give `quantity`, or `notional` together with a price")]
    CannotSize,
}

/// Parse `args` into an [`Order`] tagged for [`Venue::RobinhoodMcp`].
///
/// `default_max_slippage` is stamped on the order when the arguments do not
/// carry one; pass `Decimal::ZERO` to mean "no explicit tolerance".
pub fn parse_order(
    tool_name: &str,
    args: &Value,
    default_max_slippage: Decimal,
) -> Result<Order, OrderParseError> {
    let obj = args.as_object().ok_or(OrderParseError::NotAnObject)?;

    let symbol = obj
        .get("symbol")
        .or_else(|| obj.get("ticker"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(OrderParseError::MissingField("symbol"))?
        .to_ascii_uppercase();

    let side = match obj
        .get("side")
        .or_else(|| obj.get("action"))
        .and_then(Value::as_str)
        .ok_or(OrderParseError::MissingField("side"))?
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "buy" | "bought" | "long" => Side::Buy,
        "sell" | "sold" | "short" => Side::Sell,
        other => return Err(OrderParseError::UnknownSide(other.to_string())),
    };

    let limit_price = optional_decimal(obj, &["limit_price", "price", "limit"])?;

    let quantity = optional_decimal(obj, &["quantity", "qty", "shares", "amount"])?;
    let notional = optional_decimal(obj, &["notional", "notional_value", "dollars"])?;

    let qty = match (quantity, notional, limit_price) {
        (Some(q), _, _) => q,
        (None, Some(n), Some(p)) if p > Decimal::ZERO => n / p,
        (None, Some(_), _) => return Err(OrderParseError::CannotSize),
        (None, None, _) => return Err(OrderParseError::MissingField("quantity")),
    };
    if qty <= Decimal::ZERO {
        return Err(OrderParseError::BadField {
            field: "quantity",
            detail: format!("must be positive, got {qty}"),
        });
    }

    let max_slippage = optional_decimal(obj, &["max_slippage", "slippage"])?
        .unwrap_or(default_max_slippage)
        .max(Decimal::ZERO);

    Ok(Order {
        id: OrderId::new(next_id()),
        asset: Asset::symbol(symbol),
        side,
        qty,
        limit_price,
        max_slippage,
        venue: Venue::RobinhoodMcp,
        reason: format!("agent tool call: {tool_name}"),
    })
}

/// First key present among `keys`, parsed as a [`Decimal`]. `Ok(None)` if none
/// are present; `Err` if a present one will not parse.
fn optional_decimal(
    obj: &serde_json::Map<String, Value>,
    keys: &[&'static str],
) -> Result<Option<Decimal>, OrderParseError> {
    for &key in keys {
        if let Some(v) = obj.get(key) {
            if v.is_null() {
                continue;
            }
            let d = value_to_decimal(v).ok_or(OrderParseError::BadField {
                field: key,
                detail: format!("not a number: {v}"),
            })?;
            return Ok(Some(d));
        }
    }
    Ok(None)
}

/// Accept a JSON number, or a string like `"1.5"`, `"$50"`, `"1,000"`. Floats
/// are read through their exact textual form, never `f64`.
fn value_to_decimal(v: &Value) -> Option<Decimal> {
    match v {
        Value::Number(n) => Decimal::from_str(&n.to_string()).ok(),
        Value::String(s) => {
            let cleaned: String = s
                .trim()
                .trim_start_matches('$')
                .chars()
                .filter(|c| *c != ',' && *c != '_')
                .collect();
            Decimal::from_str(cleaned.trim()).ok()
        }
        _ => None,
    }
}

fn next_id() -> String {
    let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    format!("hook-{nanos}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use serde_json::json;

    fn parse(v: serde_json::Value) -> Result<Order, OrderParseError> {
        parse_order("place_order", &v, Decimal::ZERO)
    }

    #[test]
    fn parses_an_explicit_quantity_order() {
        let o =
            parse(json!({"symbol": "roar", "side": "Buy", "quantity": "2.5", "limit_price": "10"}))
                .unwrap();
        assert_eq!(o.asset.symbol, "ROAR");
        assert_eq!(o.side, Side::Buy);
        assert_eq!(o.qty, dec!(2.5));
        assert_eq!(o.limit_price, Some(dec!(10)));
        assert_eq!(o.venue, Venue::RobinhoodMcp);
    }

    #[test]
    fn sizes_a_notional_order_from_the_price() {
        let o = parse(json!({"symbol": "ROAR", "side": "buy", "notional": "$100", "price": "40"}))
            .unwrap();
        assert_eq!(o.qty, dec!(2.5));
    }

    #[test]
    fn notional_without_a_price_cannot_be_sized() {
        let e = parse(json!({"symbol": "ROAR", "side": "buy", "notional": "100"})).unwrap_err();
        assert_eq!(e, OrderParseError::CannotSize);
    }

    #[test]
    fn accepts_a_bare_json_number() {
        let o = parse(json!({"symbol": "ROAR", "side": "sell", "quantity": 3})).unwrap();
        assert_eq!(o.qty, dec!(3));
        assert_eq!(o.side, Side::Sell);
    }

    #[test]
    fn strips_thousands_separators() {
        let o = parse(json!({"symbol": "ROAR", "side": "buy", "quantity": "1,000"})).unwrap();
        assert_eq!(o.qty, dec!(1000));
    }

    #[test]
    fn rejects_non_object() {
        assert_eq!(
            parse(json!([1, 2])).unwrap_err(),
            OrderParseError::NotAnObject
        );
        assert_eq!(
            parse(json!(null)).unwrap_err(),
            OrderParseError::NotAnObject
        );
    }

    #[test]
    fn rejects_missing_symbol_and_side() {
        assert_eq!(
            parse(json!({"side": "buy", "quantity": "1"})).unwrap_err(),
            OrderParseError::MissingField("symbol")
        );
        assert_eq!(
            parse(json!({"symbol": "ROAR", "quantity": "1"})).unwrap_err(),
            OrderParseError::MissingField("side")
        );
    }

    #[test]
    fn rejects_unknown_side() {
        assert_eq!(
            parse(json!({"symbol": "ROAR", "side": "hodl", "quantity": "1"})).unwrap_err(),
            OrderParseError::UnknownSide("hodl".into())
        );
    }

    #[test]
    fn rejects_unparseable_and_non_positive_quantity() {
        assert!(matches!(
            parse(json!({"symbol": "ROAR", "side": "buy", "quantity": "lots"})).unwrap_err(),
            OrderParseError::BadField {
                field: "quantity",
                ..
            }
        ));
        assert!(matches!(
            parse(json!({"symbol": "ROAR", "side": "buy", "quantity": "0"})).unwrap_err(),
            OrderParseError::BadField {
                field: "quantity",
                ..
            }
        ));
        assert!(matches!(
            parse(json!({"symbol": "ROAR", "side": "buy", "quantity": "-1"})).unwrap_err(),
            OrderParseError::BadField {
                field: "quantity",
                ..
            }
        ));
    }

    #[test]
    fn default_slippage_is_applied_when_absent() {
        let o = parse_order(
            "place_order",
            &json!({"symbol": "ROAR", "side": "buy", "quantity": "1"}),
            dec!(0.01),
        )
        .unwrap();
        assert_eq!(o.max_slippage, dec!(0.01));
    }
}
