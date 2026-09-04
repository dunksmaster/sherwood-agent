//! `sherwood-server` — the local control-plane HTTP API.
//!
//! Bound to loopback, one bearer token (constant-time compared), one error
//! envelope. This is the S9 skeleton: liveness plus the `PreToolUse` order
//! hook (S7) wired to a real route. RBAC roles, the PAPER/LIVE toggle, the
//! kill-switch endpoint, the WebSocket event feed, `/metrics`, generated
//! OpenAPI, and rate limiting are the S9b / S9c increments.
//!
//! Routes:
//!
//! | Method | Path | Auth | Notes |
//! |---|---|---|---|
//! | GET  | `/v1/health` | none | liveness + mode + uptime |
//! | POST | `/v1/hook/pretooluse` | bearer | allow / deny one agent tool call |

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

/// Build the router. `/v1/health` is open; everything else requires the token.
pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/v1/hook/pretooluse", post(routes::pretooluse))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::require_token,
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
    tracing::info!(%bound, mode = state.mode.as_str(), "sherwood-server listening");

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
        AppState::new(auth::ApiToken::from_value("test-token"), risk, allowlist)
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

    #[tokio::test]
    async fn health_needs_no_auth() {
        let resp = router(test_state())
            .oneshot(Request::get("/v1/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(body_string(resp).await.contains("\"mode\":\"paper\""));
    }

    #[tokio::test]
    async fn hook_route_rejects_a_missing_token() {
        let resp = router(test_state())
            .oneshot(
                Request::post("/v1/hook/pretooluse")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let b = body_string(resp).await;
        assert!(b.contains("\"code\":\"unauthorized\""));
        assert!(b.contains("correlation_id"));
    }

    #[tokio::test]
    async fn hook_route_rejects_a_wrong_token() {
        let resp = router(test_state())
            .oneshot(
                Request::post("/v1/hook/pretooluse")
                    .header("authorization", "Bearer nope")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn hook_route_allows_a_clean_order_with_a_valid_token() {
        let req_body = serde_json::json!({
            "tool_call": {
                "name": "place_order",
                "arguments": { "symbol": "ROAR", "side": "buy", "quantity": "1", "limit_price": "100" }
            },
            "context": ctx_json()
        });
        let resp = router(test_state())
            .oneshot(
                Request::post("/v1/hook/pretooluse")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(req_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_string(resp).await, r#"{"decision":"allow"}"#);
    }

    #[tokio::test]
    async fn hook_route_denies_an_unlisted_tool_with_200() {
        let req_body = serde_json::json!({
            "tool_call": { "name": "wire_transfer", "arguments": {} },
            "context": ctx_json()
        });
        let resp = router(test_state())
            .oneshot(
                Request::post("/v1/hook/pretooluse")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(req_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // A denied tool call is a successful evaluation, not an HTTP error.
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(body_string(resp).await.contains(r#""decision":"deny""#));
    }

    #[tokio::test]
    async fn hook_route_400s_a_malformed_body_through_the_envelope() {
        let resp = router(test_state())
            .oneshot(
                Request::post("/v1/hook/pretooluse")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"tool_call": {"name": "x"}}"#)) // no context
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(body_string(resp).await.contains("\"code\":\"bad_request\""));
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
