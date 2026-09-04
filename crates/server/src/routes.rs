//! Route handlers.
//!
//! | Method | Path | Min role | Notes |
//! |---|---|---|---|
//! | `GET`  | `/v1/health` | none | liveness, mode, kill-switch, uptime |
//! | `GET`  | `/v1/control` | viewer | current mode + kill-switch |
//! | `POST` | `/v1/hook/pretooluse` | operator | allow / deny one agent tool call |
//! | `POST` | `/v1/mode` | admin + body re-auth | switch PAPER / LIVE |
//! | `POST` | `/v1/kill` | admin + body re-auth | engage / release the kill switch |
//!
//! A *denied* tool call is a `200` with `{"decision":"deny",…}` — the caller
//! (the agent's `PreToolUse` hook script) maps the body onto the CLI's
//! permission schema. Only a malformed request is a `4xx`.

use axum::extract::State;
use axum::Json;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sherwood_core::{GateContext, Portfolio};
use sherwood_execution::{HookGate, HookOutcome, ToolCall};

use crate::auth::{Caller, Role};
use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, Mode};

#[derive(Serialize)]
pub struct Health {
    status: &'static str,
    mode: &'static str,
    kill_switch: bool,
    uptime_secs: i64,
}

pub async fn health(State(state): State<AppState>) -> Json<Health> {
    let control = state.control.read().await;
    Json(Health {
        status: "ok",
        mode: control.mode.as_str(),
        kill_switch: control.kill_switch(),
        uptime_secs: state.uptime_secs(),
    })
}

#[derive(Serialize)]
pub struct ControlView {
    mode: Mode,
    kill_switch: bool,
}

pub async fn get_control(
    State(state): State<AppState>,
    caller: Caller,
) -> ApiResult<Json<ControlView>> {
    caller.require(Role::Viewer)?;
    let control = state.control.read().await;
    Ok(Json(ControlView {
        mode: control.mode,
        kill_switch: control.kill_switch(),
    }))
}

/// The market / account picture the caller has already gathered, used to build
/// a [`GateContext`]. Monetary fields are strings (`Decimal`).
#[derive(Deserialize)]
pub struct HookContext {
    /// The current portfolio, serialised exactly as `sherwood-core`'s
    /// `Portfolio` (`cash`, `positions`, `realized_pnl`, `avg_cost`).
    pub portfolio: Portfolio,
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
    caller: Caller,
    body: Json<serde_json::Value>,
) -> ApiResult<Json<HookOutcome>> {
    caller.require(Role::Operator)?;

    // Deserialise by hand so a bad body renders through our envelope, not
    // axum's default plain-text rejection.
    let req: HookRequest = serde_json::from_value(body.0)
        .map_err(|e| ApiError::bad_request(format!("invalid hook request: {e}")))?;

    let control = state.control.read().await;
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

    let outcome = HookGate::new(&state.allowlist, &control.risk).evaluate(&req.tool_call, &ctx);
    match &outcome {
        HookOutcome::Allow => tracing::info!(tool = %req.tool_call.name, "hook: allow"),
        HookOutcome::Deny { reason } => {
            tracing::warn!(tool = %req.tool_call.name, reason, "hook: deny")
        }
    }
    Ok(Json(outcome))
}

/// Fields common to the privileged toggles: the admin token, again.
fn check_reauth(state: &AppState, reauth: &str) -> ApiResult<()> {
    if state.tokens.is_admin(reauth) {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "re-authentication failed: `reauth` must be the admin token",
        ))
    }
}

#[derive(Deserialize)]
pub struct ModeRequest {
    pub mode: Mode,
    pub reauth: String,
}

pub async fn post_mode(
    State(state): State<AppState>,
    caller: Caller,
    Json(req): Json<ModeRequest>,
) -> ApiResult<Json<ControlView>> {
    caller.require(Role::Admin)?;
    check_reauth(&state, &req.reauth)?;

    if req.mode == Mode::Live && !state.allow_live {
        return Err(ApiError::forbidden(
            "live mode is disabled in config (`[server] allow_live = false`)",
        ));
    }

    let mut control = state.control.write().await;
    if control.mode != req.mode {
        tracing::warn!(
            from = control.mode.as_str(),
            to = req.mode.as_str(),
            "mode changed"
        );
        control.mode = req.mode;
    }
    Ok(Json(ControlView {
        mode: control.mode,
        kill_switch: control.kill_switch(),
    }))
}

#[derive(Deserialize)]
pub struct KillRequest {
    pub engage: bool,
    pub reauth: String,
}

pub async fn post_kill(
    State(state): State<AppState>,
    caller: Caller,
    Json(req): Json<KillRequest>,
) -> ApiResult<Json<ControlView>> {
    caller.require(Role::Admin)?;
    check_reauth(&state, &req.reauth)?;

    let mut control = state.control.write().await;
    control.risk.config_mut().kill_switch = req.engage;
    if req.engage {
        tracing::warn!("kill switch ENGAGED — every order will be rejected");
    } else {
        tracing::warn!("kill switch released");
    }
    Ok(Json(ControlView {
        mode: control.mode,
        kill_switch: control.kill_switch(),
    }))
}
