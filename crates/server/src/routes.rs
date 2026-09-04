//! Route handlers.
//!
//! | Method | Path | Min role | Notes |
//! |---|---|---|---|
//! | `GET`  | `/v1/health` | none | liveness, mode, kill-switch, uptime |
//! | `GET`  | `/v1/metrics` | none | Prometheus text |
//! | `GET`  | `/v1/control` | viewer | current mode + kill-switch |
//! | `GET`  | `/v1/portfolio` | viewer | last persisted portfolio snapshot |
//! | `GET`  | `/v1/activity` | viewer | recent audit events + fill count |
//! | `GET`  | `/v1/audit/verify` | viewer | recompute the audit hash chain |
//! | `GET`  | `/v1/events` | viewer | SSE — new audit-chain rows as they land |
//! | `GET`  | `/v1/approvals` | viewer | the approval queue + mode |
//! | `POST` | `/v1/approvals/{id}` | operator | approve / deny a pending order |
//! | `POST` | `/v1/hook/pretooluse` | operator | allow / deny one agent tool call |
//! | `POST` | `/v1/mode` | admin + body re-auth | switch PAPER / LIVE |
//! | `POST` | `/v1/kill` | admin + body re-auth | engage / release the kill switch |
//!
//! A *denied* tool call is a `200` with `{"decision":"deny",…}` — the caller
//! (the agent's `PreToolUse` hook script) maps the body onto the CLI's
//! permission schema. Only a malformed request is a `4xx`.
//!
//! The read-only views serve whatever `sherwood run` last persisted to the same
//! `state_path`; without one configured they answer `404`.

use axum::extract::{Path, Query, State};
use axum::http::header::CONTENT_TYPE;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sherwood_core::{Asset, GateContext, Portfolio};
use sherwood_execution::order_parse::parse_order;
use sherwood_execution::{HookGate, HookOutcome, ToolCall, ToolClass};
use sherwood_store::{AuditEvent, AuditVerification, SqliteStore, Store};

use crate::approvals::{Approval, ApprovalMode, ApprovalState};
use std::convert::Infallible;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::wrappers::IntervalStream;
use tokio_stream::{Stream, StreamExt};

use crate::auth::{Caller, Role};
use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, Mode};

fn store(state: &AppState) -> ApiResult<&SqliteStore> {
    state
        .store
        .as_deref()
        .ok_or_else(|| ApiError::not_found("no state_path is configured; nothing to read"))
}

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

/// `GET /v1/metrics` — Prometheus text. Open, like `/v1/health`; the server is
/// loopback-only.
pub async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    let (mode, kill) = {
        let c = state.control.read().await;
        (c.mode, c.kill_switch())
    };
    let extra = format!(
        "# HELP sherwood_kill_switch 1 if the kill switch is engaged.\n\
         # TYPE sherwood_kill_switch gauge\n\
         sherwood_kill_switch {}\n\
         # HELP sherwood_mode_live 1 if the server is in LIVE mode.\n\
         # TYPE sherwood_mode_live gauge\n\
         sherwood_mode_live {}\n",
        u8::from(kill),
        u8::from(mode == Mode::Live),
    );
    (
        [(CONTENT_TYPE, "text/plain; version=0.0.4")],
        state.metrics.render(&extra),
    )
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
    drop(control); // release the risk-config read lock before any await

    // In `manual` mode a risk-passing *order* is held for the operator. Reads,
    // cancels, and anything the gate already denied are unaffected.
    if matches!(outcome, HookOutcome::Allow)
        && state.approval_mode == ApprovalMode::Manual
        && state.allowlist.classify(&req.tool_call.name) == Some(ToolClass::PlaceOrder)
    {
        if let Ok(order) = parse_order(&req.tool_call.name, &req.tool_call.arguments, Decimal::ZERO)
        {
            let ticket = state.approvals.enqueue(&req.tool_call.name, &order);
            tracing::info!(id = %ticket.id, tool = %req.tool_call.name, "hook: order held for approval");
            let decided = ticket.wait().await;
            let held = match decided {
                ApprovalState::Approved => HookOutcome::Allow,
                ApprovalState::Denied => HookOutcome::Deny {
                    reason: "operator denied the order".into(),
                },
                ApprovalState::Expired | ApprovalState::Pending => HookOutcome::Deny {
                    reason: "approval timed out".into(),
                },
            };
            tracing::info!(?decided, "hook: approval resolved");
            return Ok(Json(held));
        }
    }

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

// ---- read-only views over the persisted state ----

#[derive(Serialize)]
pub struct PositionView {
    symbol: String,
    quantity: Decimal,
    avg_cost: Option<Decimal>,
}

#[derive(Serialize)]
pub struct PortfolioView {
    cash: Decimal,
    realized_pnl: Decimal,
    open_positions: usize,
    positions: Vec<PositionView>,
}

pub async fn get_portfolio(
    State(state): State<AppState>,
    caller: Caller,
) -> ApiResult<Json<PortfolioView>> {
    caller.require(Role::Viewer)?;
    let pf = store(&state)?
        .load_portfolio()
        .await
        .map_err(|e| ApiError::internal(format!("store: {e}")))?
        .ok_or_else(|| ApiError::not_found("no portfolio snapshot has been written yet"))?;

    let mut positions: Vec<PositionView> = pf
        .positions()
        .map(|(sym, qty)| PositionView {
            symbol: sym.to_string(),
            quantity: qty,
            avg_cost: pf.avg_cost(&Asset::symbol(sym)),
        })
        .collect();
    positions.sort_by(|a, b| a.symbol.cmp(&b.symbol));

    Ok(Json(PortfolioView {
        cash: pf.cash(),
        realized_pnl: pf.realized_pnl(),
        open_positions: pf.open_position_count(),
        positions,
    }))
}

#[derive(Deserialize)]
pub struct ActivityQuery {
    #[serde(default = "default_activity_limit")]
    limit: i64,
}

fn default_activity_limit() -> i64 {
    50
}

#[derive(Serialize)]
pub struct ActivityView {
    /// Recent audit-chain events, oldest first.
    recent: Vec<AuditEvent>,
    /// Total fills recorded.
    fills: usize,
}

pub async fn get_activity(
    State(state): State<AppState>,
    caller: Caller,
    Query(q): Query<ActivityQuery>,
) -> ApiResult<Json<ActivityView>> {
    caller.require(Role::Viewer)?;
    let s = store(&state)?;
    let limit = q.limit.clamp(1, 500);
    let recent = s
        .audit_tail(limit)
        .await
        .map_err(|e| ApiError::internal(format!("store: {e}")))?;
    let fills = s
        .fills()
        .await
        .map_err(|e| ApiError::internal(format!("store: {e}")))?
        .len();
    Ok(Json(ActivityView { recent, fills }))
}

#[derive(Serialize)]
pub struct AuditVerifyView {
    ok: bool,
    entries: Option<i64>,
    broken_at: Option<i64>,
}

pub async fn get_audit_verify(
    State(state): State<AppState>,
    caller: Caller,
) -> ApiResult<Json<AuditVerifyView>> {
    caller.require(Role::Viewer)?;
    let v = store(&state)?
        .verify_audit_chain()
        .await
        .map_err(|e| ApiError::internal(format!("store: {e}")))?;
    Ok(Json(match v {
        AuditVerification::Ok { entries } => AuditVerifyView {
            ok: true,
            entries: Some(entries),
            broken_at: None,
        },
        AuditVerification::Broken { at_seq, .. } => {
            tracing::error!(at_seq, "audit chain verification FAILED");
            AuditVerifyView {
                ok: false,
                entries: None,
                broken_at: Some(at_seq),
            }
        }
    }))
}

// ---- approval gate ----

#[derive(Serialize)]
pub struct ApprovalsView {
    mode: ApprovalMode,
    pending: usize,
    approvals: Vec<Approval>,
}

pub async fn get_approvals(
    State(state): State<AppState>,
    caller: Caller,
) -> ApiResult<Json<ApprovalsView>> {
    caller.require(Role::Viewer)?;
    Ok(Json(ApprovalsView {
        mode: state.approval_mode,
        pending: state.approvals.pending_count(),
        approvals: state.approvals.list(),
    }))
}

#[derive(Deserialize)]
pub struct DecisionRequest {
    /// `"approve"` or `"deny"`.
    pub decision: String,
    #[serde(default)]
    pub reason: Option<String>,
}

pub async fn post_approval(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    Json(req): Json<DecisionRequest>,
) -> ApiResult<Json<Approval>> {
    caller.require(Role::Operator)?;
    let target = match req.decision.as_str() {
        "approve" => ApprovalState::Approved,
        "deny" => ApprovalState::Denied,
        other => {
            return Err(ApiError::bad_request(format!(
                "decision must be \"approve\" or \"deny\", got {other:?}"
            )))
        }
    };
    let changed = state
        .approvals
        .decide(&id, target, req.reason)
        .map_err(ApiError::bad_request)?;
    if !changed {
        return Err(ApiError::not_found(format!(
            "approval {id} is unknown or already decided"
        )));
    }
    tracing::info!(%id, ?target, role = ?caller.0, "approval decided");
    state
        .approvals
        .list()
        .into_iter()
        .find(|a| a.id == id)
        .map(Json)
        .ok_or_else(|| ApiError::internal("approval vanished after decision"))
}

/// How often the SSE stream polls the store for new audit rows.
const EVENTS_POLL: Duration = Duration::from_millis(1500);

/// `GET /v1/events` — a Server-Sent Events stream. Each tick emits an `audit`
/// event whose data is a JSON array of audit-chain rows appended since the last
/// tick (an empty array when nothing changed — the connection also gets a
/// keep-alive comment). Read-only and one-directional: exactly the shape of an
/// activity feed, and it rides the same auth middleware as every other route.
///
/// SSE rather than a WebSocket because the feed only ever flows server→client,
/// browsers reconnect it natively, and there is no upgrade handshake to secure
/// separately. When the run loop is later hosted in-process it can push live
/// `sherwood-events` onto this same stream.
pub async fn get_events(
    State(state): State<AppState>,
    caller: Caller,
) -> ApiResult<Sse<impl Stream<Item = Result<SseEvent, Infallible>>>> {
    caller.require(Role::Viewer)?;

    let store = state.store.clone();
    let last_seq = Arc::new(AtomicI64::new(0));

    let stream = IntervalStream::new(tokio::time::interval(EVENTS_POLL)).map(move |_| {
        let store = store.clone();
        let last_seq = last_seq.clone();
        (store, last_seq)
    });
    // `then` turns each tick into the async DB read.
    let stream = stream.then(|(store, last_seq)| async move {
        let rows = match &store {
            Some(s) => s.audit_tail(200).await.unwrap_or_default(),
            None => Vec::new(),
        };
        let since = last_seq.load(Ordering::Relaxed);
        let fresh: Vec<AuditEvent> = rows.into_iter().filter(|r| r.seq > since).collect();
        if let Some(max) = fresh.iter().map(|r| r.seq).max() {
            last_seq.store(max, Ordering::Relaxed);
        }
        let data = serde_json::to_string(&fresh).unwrap_or_else(|_| "[]".to_string());
        Ok(SseEvent::default().event("audit").data(data))
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
