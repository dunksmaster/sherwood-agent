//! Route handlers.
//!
//! Two so far:
//!
//! * `GET  /v1/health` — unauthenticated liveness.
//! * `POST /v1/hook/pretooluse` — the fail-closed order gate (S7). Auth
//!   required. A *denied* tool call is a `200` with `{"decision":"deny",…}` in
//!   the body, not an HTTP error — the caller (the agent's `PreToolUse` hook
//!   script) maps the body onto the CLI's permission schema. Only a malformed
//!   request is a `4xx`.

use axum::extract::State;
use axum::Json;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sherwood_core::{GateContext, Portfolio};
use sherwood_execution::{HookGate, HookOutcome, ToolCall};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Serialize)]
pub struct Health {
    status: &'static str,
    mode: &'static str,
    uptime_secs: i64,
}

pub async fn health(State(state): State<AppState>) -> Json<Health> {
    Json(Health {
        status: "ok",
        mode: state.mode.as_str(),
        uptime_secs: state.uptime_secs(),
    })
}

/// The market / account picture the caller has already gathered, used to build
/// a [`GateContext`]. Monetary fields are strings (`Decimal`).
#[derive(Deserialize)]
pub struct HookContext {
    /// The current portfolio, serialised exactly as `sherwood-core`'s
    /// `Portfolio` (`cash`, `positions`, `realized_pnl`, `avg_cost`).
    pub portfolio: Portfolio,
    /// Best available mark for the order's asset. Falls back to the order's own
    /// limit price inside the gate when absent.
    #[serde(default)]
    pub ref_price: Option<Decimal>,
    pub equity: Decimal,
    pub unrealized_pnl: Decimal,
    #[serde(default)]
    pub last_order_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
pub struct HookRequest {
    pub tool_call: ToolCall,
    pub context: HookContext,
}

pub async fn pretooluse(
    State(state): State<AppState>,
    body: Json<serde_json::Value>,
) -> ApiResult<Json<HookOutcome>> {
    // Deserialise by hand so a bad body renders through our envelope, not
    // axum's default plain-text rejection.
    let req: HookRequest = serde_json::from_value(body.0)
        .map_err(|e| ApiError::bad_request(format!("invalid hook request: {e}")))?;

    let ctx = GateContext {
        portfolio: &req.context.portfolio,
        ref_price: req.context.ref_price,
        equity: req.context.equity,
        unrealized_pnl: req.context.unrealized_pnl,
        last_order_at: req.context.last_order_at,
        // The API boundary may read the wall clock; the gate still receives it
        // explicitly and never reads it itself.
        now: Utc::now(),
    };

    let outcome = HookGate::new(&state.allowlist, &state.risk).evaluate(&req.tool_call, &ctx);
    match &outcome {
        HookOutcome::Allow => tracing::info!(tool = %req.tool_call.name, "hook: allow"),
        HookOutcome::Deny { reason } => {
            tracing::warn!(tool = %req.tool_call.name, reason, "hook: deny")
        }
    }
    Ok(Json(outcome))
}
