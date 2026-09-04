//! The `PreToolUse` approval hook — the fail-closed choke point for
//! [ADR-0001](../../../docs/adr/0001-mcp-interaction-model.md) Option 3.
//!
//! Under Option 3 a headless `claude` / `codex` agent holds the Robinhood MCP
//! connection directly. Every tool call it makes is intercepted by a
//! `PreToolUse` hook, which posts the pending call to `sherwood-server`; the
//! server calls [`HookGate::evaluate`] and returns allow or deny to the hook,
//! which the hook translates into the agent CLI's own permission schema.
//!
//! This module is the whole decision. It is the only thing between the agent
//! and the venue, so the contract is deliberately paranoid:
//!
//! * a tool that is not on the [`ToolAllowlist`] is **denied**;
//! * an order-placing call whose arguments do not parse cleanly is **denied**,
//!   never passed through;
//! * a parsed order is denied unless it passes [`RiskGate::check`] unchanged;
//! * only read-only and cancel tools are allowed without a risk check — a
//!   cancel can only reduce exposure, and must keep working during a halt.
//!
//! The transport (an axum route) and the agent-process supervision arrive with
//! S9 and later; this is pure and synchronous so it can be tested in isolation.

use crate::order_parse::parse_order;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sherwood_core::{GateContext, RiskGate};
use std::collections::HashMap;

/// One pending tool invocation, as forwarded by the `PreToolUse` hook.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolCall {
    pub name: String,
    /// The tool arguments, verbatim. Shape depends on the tool; an
    /// order-placing tool is expected to carry an object (see [`parse_order`]).
    #[serde(default)]
    pub arguments: serde_json::Value,
}

/// How a permitted tool is treated by the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolClass {
    /// Reads only — accounts, positions, quotes, order history. Always allowed.
    ReadOnly,
    /// Places or replaces an order. Parsed and risk-checked before allow.
    PlaceOrder,
    /// Cancels a resting order. Allowed without a risk check — it only ever
    /// reduces exposure, and must work even when a hard stop is engaged.
    CancelOrder,
}

/// The set of MCP tools the agent is permitted to call, and how each is
/// classified. Anything not present here is denied.
///
/// The library ships **no** Robinhood tool names baked in: the exact names the
/// Robinhood MCP exposes are an open item on ADR-0001 (confirm them from
/// `claude mcp` output at wiring time), and pinning a guess in code would be a
/// latent security bug the day the names change. Build the allowlist from
/// configuration.
#[derive(Debug, Clone, Default)]
pub struct ToolAllowlist {
    tools: HashMap<String, ToolClass>,
}

impl ToolAllowlist {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from `(tool_name, class)` pairs.
    pub fn from_pairs<I, S>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (S, ToolClass)>,
        S: Into<String>,
    {
        Self {
            tools: pairs.into_iter().map(|(n, c)| (n.into(), c)).collect(),
        }
    }

    /// Add or replace one entry.
    pub fn allow(&mut self, name: impl Into<String>, class: ToolClass) -> &mut Self {
        self.tools.insert(name.into(), class);
        self
    }

    /// The class of `name`, or `None` if it is not allowlisted.
    pub fn classify(&self, name: &str) -> Option<ToolClass> {
        self.tools.get(name).copied()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }
}

/// The hook's answer for one tool call. Serialises to
/// `{"decision":"allow"}` or `{"decision":"deny","reason":"…"}`; the hook
/// script maps that onto the agent CLI's permission schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "decision", rename_all = "lowercase")]
pub enum HookOutcome {
    Allow,
    Deny { reason: String },
}

impl HookOutcome {
    fn deny(reason: impl Into<String>) -> Self {
        Self::Deny {
            reason: reason.into(),
        }
    }

    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }

    /// The denial reason, if this is a denial.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Allow => None,
            Self::Deny { reason } => Some(reason),
        }
    }
}

/// Evaluates one intercepted tool call against the allowlist and the risk gate.
pub struct HookGate<'g> {
    allowlist: &'g ToolAllowlist,
    gate: &'g RiskGate,
    /// `max_slippage` stamped on an order the agent did not specify one for.
    /// Zero means "no explicit tolerance", which never trips the slippage cap.
    default_max_slippage: Decimal,
}

impl<'g> HookGate<'g> {
    pub fn new(allowlist: &'g ToolAllowlist, gate: &'g RiskGate) -> Self {
        Self {
            allowlist,
            gate,
            default_max_slippage: Decimal::ZERO,
        }
    }

    #[must_use]
    pub fn with_default_max_slippage(mut self, slippage: Decimal) -> Self {
        self.default_max_slippage = slippage;
        self
    }

    /// Allow or deny `call`. Every non-clean path returns [`HookOutcome::Deny`].
    pub fn evaluate(&self, call: &ToolCall, ctx: &GateContext<'_>) -> HookOutcome {
        let Some(class) = self.allowlist.classify(&call.name) else {
            return HookOutcome::deny(format!("tool `{}` is not on the allowlist", call.name));
        };

        match class {
            ToolClass::ReadOnly | ToolClass::CancelOrder => HookOutcome::Allow,
            ToolClass::PlaceOrder => self.evaluate_order(call, ctx),
        }
    }

    fn evaluate_order(&self, call: &ToolCall, ctx: &GateContext<'_>) -> HookOutcome {
        let order = match parse_order(&call.name, &call.arguments, self.default_max_slippage) {
            Ok(order) => order,
            Err(e) => {
                // Fail closed: an order call we cannot fully understand is denied.
                return HookOutcome::deny(format!("rejected order tool call: {e}"));
            }
        };

        match self.gate.check(&order, ctx) {
            Ok(()) => HookOutcome::Allow,
            Err(reject) => HookOutcome::deny(format!("risk gate: {reject}")),
        }
    }
}

/// Convenience: parse a JSON hook payload and evaluate it in one call. A body
/// that is not a valid [`ToolCall`] is denied.
pub fn evaluate_payload(
    payload: &str,
    allowlist: &ToolAllowlist,
    gate: &RiskGate,
    ctx: &GateContext<'_>,
) -> HookOutcome {
    match serde_json::from_str::<ToolCall>(payload) {
        Ok(call) => HookGate::new(allowlist, gate).evaluate(&call, ctx),
        Err(e) => HookOutcome::Deny {
            reason: format!("unparseable hook payload: {e}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rust_decimal_macros::dec;
    use serde_json::json;
    use sherwood_core::{Portfolio, RiskConfig};

    fn allowlist() -> ToolAllowlist {
        ToolAllowlist::from_pairs([
            ("get_positions", ToolClass::ReadOnly),
            ("get_quote", ToolClass::ReadOnly),
            ("place_order", ToolClass::PlaceOrder),
            ("cancel_order", ToolClass::CancelOrder),
        ])
    }

    fn gate(cfg: RiskConfig) -> RiskGate {
        RiskGate::new(cfg)
    }

    fn permissive_cfg() -> RiskConfig {
        RiskConfig {
            max_order_notional: dec!(10_000),
            max_position_fraction: dec!(1),
            ..RiskConfig::default()
        }
    }

    fn ctx<'a>(portfolio: &'a Portfolio, price: Decimal) -> GateContext<'a> {
        GateContext {
            portfolio,
            ref_price: Some(price),
            equity: dec!(1_000),
            unrealized_pnl: dec!(0),
            last_order_at: None,
            now: Utc::now(),
        }
    }

    fn call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            name: name.into(),
            arguments: args,
        }
    }

    #[test]
    fn unknown_tool_is_denied() {
        let (al, g, pf) = (
            allowlist(),
            gate(permissive_cfg()),
            Portfolio::new(dec!(1_000)),
        );
        let out = HookGate::new(&al, &g)
            .evaluate(&call("transfer_funds", json!({})), &ctx(&pf, dec!(100)));
        assert_eq!(
            out.reason(),
            Some("tool `transfer_funds` is not on the allowlist")
        );
    }

    #[test]
    fn read_only_tool_is_allowed() {
        let (al, g, pf) = (
            allowlist(),
            gate(permissive_cfg()),
            Portfolio::new(dec!(1_000)),
        );
        let out = HookGate::new(&al, &g)
            .evaluate(&call("get_positions", json!({})), &ctx(&pf, dec!(100)));
        assert_eq!(out, HookOutcome::Allow);
    }

    #[test]
    fn cancel_is_allowed_even_with_kill_switch_engaged() {
        let cfg = RiskConfig {
            kill_switch: true,
            ..permissive_cfg()
        };
        let (al, g, pf) = (allowlist(), gate(cfg), Portfolio::new(dec!(1_000)));
        let out = HookGate::new(&al, &g).evaluate(
            &call("cancel_order", json!({"id": "x"})),
            &ctx(&pf, dec!(100)),
        );
        assert_eq!(out, HookOutcome::Allow);
    }

    #[test]
    fn valid_order_within_caps_is_allowed() {
        let (al, g, pf) = (
            allowlist(),
            gate(permissive_cfg()),
            Portfolio::new(dec!(1_000)),
        );
        let args = json!({"symbol": "ROAR", "side": "buy", "quantity": "1", "limit_price": "100"});
        let out = HookGate::new(&al, &g).evaluate(&call("place_order", args), &ctx(&pf, dec!(100)));
        assert_eq!(out, HookOutcome::Allow);
    }

    #[test]
    fn order_over_the_notional_cap_is_denied_with_the_gate_reason() {
        let cfg = RiskConfig {
            max_order_notional: dec!(50),
            ..permissive_cfg()
        };
        let (al, g, pf) = (allowlist(), gate(cfg), Portfolio::new(dec!(1_000)));
        let args = json!({"symbol": "ROAR", "side": "buy", "quantity": "1", "limit_price": "100"});
        let out = HookGate::new(&al, &g).evaluate(&call("place_order", args), &ctx(&pf, dec!(100)));
        assert!(out.reason().unwrap().contains("notional"), "{out:?}");
    }

    #[test]
    fn order_with_kill_switch_is_denied() {
        let cfg = RiskConfig {
            kill_switch: true,
            ..permissive_cfg()
        };
        let (al, g, pf) = (allowlist(), gate(cfg), Portfolio::new(dec!(1_000)));
        let args = json!({"symbol": "ROAR", "side": "buy", "quantity": "1", "limit_price": "100"});
        let out = HookGate::new(&al, &g).evaluate(&call("place_order", args), &ctx(&pf, dec!(100)));
        assert!(out.reason().unwrap().contains("kill switch"), "{out:?}");
    }

    #[test]
    fn denylisted_symbol_is_denied() {
        let mut cfg = permissive_cfg();
        cfg.denylist.insert("SCAM".into());
        let (al, g, pf) = (allowlist(), gate(cfg), Portfolio::new(dec!(1_000)));
        let args = json!({"symbol": "SCAM", "side": "buy", "quantity": "1", "limit_price": "10"});
        let out = HookGate::new(&al, &g).evaluate(&call("place_order", args), &ctx(&pf, dec!(10)));
        assert!(out.reason().unwrap().contains("denylist"), "{out:?}");
    }

    #[test]
    fn order_missing_symbol_is_denied_not_passed_through() {
        let (al, g, pf) = (
            allowlist(),
            gate(permissive_cfg()),
            Portfolio::new(dec!(1_000)),
        );
        let args = json!({"side": "buy", "quantity": "1", "limit_price": "100"});
        let out = HookGate::new(&al, &g).evaluate(&call("place_order", args), &ctx(&pf, dec!(100)));
        assert!(out.reason().unwrap().contains("symbol"), "{out:?}");
    }

    #[test]
    fn order_with_non_object_arguments_is_denied() {
        let (al, g, pf) = (
            allowlist(),
            gate(permissive_cfg()),
            Portfolio::new(dec!(1_000)),
        );
        for bad in [json!(null), json!([1, 2, 3]), json!("place ROAR")] {
            let out = HookGate::new(&al, &g)
                .evaluate(&call("place_order", bad.clone()), &ctx(&pf, dec!(100)));
            assert!(!out.is_allowed(), "{bad} should be denied");
        }
    }

    #[test]
    fn order_with_unknown_side_is_denied() {
        let (al, g, pf) = (
            allowlist(),
            gate(permissive_cfg()),
            Portfolio::new(dec!(1_000)),
        );
        let args = json!({"symbol": "ROAR", "side": "yolo", "quantity": "1", "limit_price": "100"});
        let out = HookGate::new(&al, &g).evaluate(&call("place_order", args), &ctx(&pf, dec!(100)));
        assert!(!out.is_allowed());
    }

    #[test]
    fn order_with_non_numeric_quantity_is_denied() {
        let (al, g, pf) = (
            allowlist(),
            gate(permissive_cfg()),
            Portfolio::new(dec!(1_000)),
        );
        let args =
            json!({"symbol": "ROAR", "side": "buy", "quantity": "lots", "limit_price": "100"});
        let out = HookGate::new(&al, &g).evaluate(&call("place_order", args), &ctx(&pf, dec!(100)));
        assert!(!out.is_allowed());
    }

    #[test]
    fn empty_allowlist_denies_everything() {
        let (al, g, pf) = (
            ToolAllowlist::new(),
            gate(permissive_cfg()),
            Portfolio::new(dec!(1_000)),
        );
        let hg = HookGate::new(&al, &g);
        for name in ["place_order", "get_positions", "cancel_order"] {
            assert!(!hg
                .evaluate(&call(name, json!({})), &ctx(&pf, dec!(100)))
                .is_allowed());
        }
    }

    #[test]
    fn evaluate_payload_denies_a_malformed_body() {
        let (al, g, pf) = (
            allowlist(),
            gate(permissive_cfg()),
            Portfolio::new(dec!(1_000)),
        );
        let out = evaluate_payload("{not json", &al, &g, &ctx(&pf, dec!(100)));
        assert!(out.reason().unwrap().contains("unparseable"), "{out:?}");
    }

    #[test]
    fn evaluate_payload_round_trips_a_valid_call() {
        let (al, g, pf) = (
            allowlist(),
            gate(permissive_cfg()),
            Portfolio::new(dec!(1_000)),
        );
        let body = r#"{"name":"place_order","arguments":{"symbol":"ROAR","side":"buy","quantity":"1","limit_price":"100"}}"#;
        assert_eq!(
            evaluate_payload(body, &al, &g, &ctx(&pf, dec!(100))),
            HookOutcome::Allow
        );
    }

    #[test]
    fn outcome_serialises_to_the_documented_shape() {
        assert_eq!(
            serde_json::to_string(&HookOutcome::Allow).unwrap(),
            r#"{"decision":"allow"}"#
        );
        assert_eq!(
            serde_json::to_string(&HookOutcome::deny("nope")).unwrap(),
            r#"{"decision":"deny","reason":"nope"}"#
        );
    }
}
