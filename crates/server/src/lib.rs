//! `sherwood-server` — the local control-plane HTTP API.
//!
//! Bound to loopback, bearer-token auth with three RBAC roles
//! (`viewer` / `operator` / `admin`), one error envelope. S9a shipped the
//! skeleton and the `PreToolUse` order hook; S9b (this) adds the roles, the
//! PAPER/LIVE toggle, and the kill-switch endpoint. The WebSocket event feed,
//! `/metrics`, generated OpenAPI, and rate limiting are S9c.
//!
//! | Method | Path | Min role | Notes |
//! |---|---|---|---|
//! | GET  | `/v1/health` | none | liveness, mode, kill-switch, uptime |
//! | GET  | `/v1/control` | viewer | current mode + kill-switch |
//! | POST | `/v1/hook/pretooluse` | operator | allow / deny one agent tool call |
//! | POST | `/v1/mode` | admin + body re-auth | switch PAPER / LIVE |
//! | POST | `/v1/kill` | admin + body re-auth | engage / release the kill switch |

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod auth;
pub mod error;
pub mod routes;
pub mod state;

pub use error::{ApiError, ApiResult};
pub use state::{AppState, Mode};

use axum::routing::{get, post};
use axum::Router;
use std::future::Future;
use std::net::SocketAddr;

/// Build the router. `/v1/health` is open; every other route runs behind
/// [`auth::require_auth`] and then checks its own minimum role.
pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/v1/control", get(routes::get_control))
        .route("/v1/hook/pretooluse", post(routes::pretooluse))
        .route("/v1/mode", post(routes::post_mode))
        .route("/v1/kill", post(routes::post_kill))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ));

    Router::new()
        .route("/v1/health", get(routes::health))
        .merge(protected)
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state)
}

/// Serve until `shutdown` resolves. Refuses any non-loopback bind — TLS for a
/// public bind is a later concern (see `docs/SECURITY.md`).
pub async fn serve<F>(addr: SocketAddr, state: AppState, shutdown: F) -> std::io::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    if !addr.ip().is_loopback() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("refusing to bind {addr}: only loopback is allowed without TLS"),
        ));
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;
    tracing::info!(%bound, "sherwood-server listening");

    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown)
        .await
}

/// A short request-correlation id (16 hex chars). Best-effort — a CSPRNG hiccup
/// degrades to a fixed placeholder rather than failing the response.
pub(crate) fn new_correlation_id() -> String {
    let mut bytes = [0u8; 8];
    match getrandom::getrandom(&mut bytes) {
        Ok(()) => hex::encode(bytes),
        Err(_) => "0000000000000000".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use rust_decimal_macros::dec;
    use sherwood_core::{RiskConfig, RiskGate};
    use sherwood_execution::{ToolAllowlist, ToolClass};
    use tower::ServiceExt;

    const ADMIN: &str = "admin-token";
    const OPERATOR: &str = "operator-token";

    fn test_state() -> AppState {
        let allowlist = ToolAllowlist::from_pairs([
            ("get_positions", ToolClass::ReadOnly),
            ("place_order", ToolClass::PlaceOrder),
        ]);
        let risk = RiskGate::new(RiskConfig {
            max_order_notional: dec!(10_000),
            max_position_fraction: dec!(1),
            ..RiskConfig::default()
        });
        let tokens = auth::TokenSet::new(
            auth::ApiToken::from_value(ADMIN),
            Some(auth::ApiToken::from_value(OPERATOR)),
            None,
        );
        AppState::new(tokens, risk, allowlist, /* allow_live */ false)
    }

    fn allow_live_state() -> AppState {
        let s = test_state();
        AppState {
            allow_live: true,
            ..s
        }
    }

    fn ctx_json() -> serde_json::Value {
        serde_json::json!({
            "portfolio": { "cash": "1000", "positions": {}, "realized_pnl": "0", "avg_cost": {} },
            "ref_price": "100",
            "equity": "1000",
            "unrealized_pnl": "0"
        })
    }

    async fn body_string(resp: axum::response::Response) -> String {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn post(path: &str, token: Option<&str>, json: serde_json::Value) -> Request<Body> {
        let mut b = Request::post(path).header("content-type", "application/json");
        if let Some(t) = token {
            b = b.header("authorization", format!("Bearer {t}"));
        }
        b.body(Body::from(json.to_string())).unwrap()
    }

    async fn call(state: AppState, req: Request<Body>) -> axum::response::Response {
        router(state).oneshot(req).await.unwrap()
    }

    #[tokio::test]
    async fn health_needs_no_auth_and_shows_kill_switch() {
        let resp = call(
            test_state(),
            Request::get("/v1/health").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let b = body_string(resp).await;
        assert!(b.contains("\"mode\":\"paper\""));
        assert!(b.contains("\"kill_switch\":false"));
    }

    #[tokio::test]
    async fn hook_rejects_missing_and_wrong_tokens() {
        for tok in [None, Some("nope")] {
            let resp = call(
                test_state(),
                post("/v1/hook/pretooluse", tok, serde_json::json!({})),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }
    }

    #[tokio::test]
    async fn operator_can_use_the_hook_but_not_the_toggles() {
        let body = serde_json::json!({
            "tool_call": { "name": "place_order",
                "arguments": { "symbol": "ROAR", "side": "buy", "quantity": "1", "limit_price": "100" } },
            "context": ctx_json()
        });
        let resp = call(
            test_state(),
            post("/v1/hook/pretooluse", Some(OPERATOR), body),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_string(resp).await, r#"{"decision":"allow"}"#);

        let resp = call(
            test_state(),
            post(
                "/v1/kill",
                Some(OPERATOR),
                serde_json::json!({ "engage": true, "reauth": ADMIN }),
            ),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_denied_tool_call_is_200_not_an_error() {
        let body = serde_json::json!({
            "tool_call": { "name": "wire_transfer", "arguments": {} },
            "context": ctx_json()
        });
        let resp = call(
            test_state(),
            post("/v1/hook/pretooluse", Some(OPERATOR), body),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(body_string(resp).await.contains(r#""decision":"deny""#));
    }

    #[tokio::test]
    async fn malformed_hook_body_is_400_through_the_envelope() {
        let resp = call(
            test_state(),
            post(
                "/v1/hook/pretooluse",
                Some(OPERATOR),
                serde_json::json!({ "tool_call": { "name": "x" } }),
            ),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(body_string(resp).await.contains("\"code\":\"bad_request\""));
    }

    #[tokio::test]
    async fn kill_switch_engaged_then_the_hook_denies_orders() {
        let state = test_state();

        let resp = call(
            state.clone(),
            post(
                "/v1/kill",
                Some(ADMIN),
                serde_json::json!({ "engage": true, "reauth": ADMIN }),
            ),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(body_string(resp).await.contains("\"kill_switch\":true"));

        let body = serde_json::json!({
            "tool_call": { "name": "place_order",
                "arguments": { "symbol": "ROAR", "side": "buy", "quantity": "1", "limit_price": "100" } },
            "context": ctx_json()
        });
        let resp = call(state, post("/v1/hook/pretooluse", Some(ADMIN), body)).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(body_string(resp).await.contains("kill switch"));
    }

    #[tokio::test]
    async fn kill_requires_body_reauth_with_the_admin_token() {
        let resp = call(
            test_state(),
            post(
                "/v1/kill",
                Some(ADMIN),
                serde_json::json!({ "engage": true, "reauth": "wrong" }),
            ),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        assert!(body_string(resp).await.contains("re-authentication failed"));
    }

    #[tokio::test]
    async fn mode_toggle_to_live_is_refused_unless_allowed_in_config() {
        let resp = call(
            test_state(),
            post(
                "/v1/mode",
                Some(ADMIN),
                serde_json::json!({ "mode": "live", "reauth": ADMIN }),
            ),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let resp = call(
            allow_live_state(),
            post(
                "/v1/mode",
                Some(ADMIN),
                serde_json::json!({ "mode": "live", "reauth": ADMIN }),
            ),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(body_string(resp).await.contains("\"mode\":\"live\""));
    }

    #[tokio::test]
    async fn non_loopback_bind_is_refused() {
        let err = serve(
            "8.8.8.8:9999".parse().unwrap(),
            test_state(),
            std::future::ready(()),
        )
        .await
        .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }
}
