//! `sherwood-server` — the local control-plane HTTP API.
//!
//! Loopback bind, bearer auth with three RBAC roles, one error envelope, a
//! global rate limit, CORS for the dashboard origin, and Prometheus metrics.
//! The WebSocket event feed and generated OpenAPI wait on S11 (they need the
//! run loop folded into the server) and route stability.
//!
//! | Method | Path | Min role | Notes |
//! |---|---|---|---|
//! | GET  | `/v1/health` | none | liveness, mode, kill-switch, uptime |
//! | GET  | `/v1/metrics` | none | Prometheus text |
//! | GET  | `/v1/control` | viewer | current mode + kill-switch |
//! | GET  | `/v1/portfolio` | viewer | last persisted portfolio snapshot |
//! | GET  | `/v1/activity` | viewer | recent audit events + fill count |
//! | GET  | `/v1/audit/verify` | viewer | recompute the audit hash chain |
//! | POST | `/v1/hook/pretooluse` | operator | allow / deny one agent tool call |
//! | POST | `/v1/mode` | admin + body re-auth | switch PAPER / LIVE |
//! | POST | `/v1/kill` | admin + body re-auth | engage / release the kill switch |

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod auth;
pub mod error;
pub mod limit;
pub mod metrics;
mod mw;
pub mod routes;
pub mod state;

pub use error::{ApiError, ApiResult};
pub use state::{AppState, Mode, ServerOpts};

use axum::http::{
    header::{AUTHORIZATION, CONTENT_TYPE},
    HeaderName, HeaderValue, Method,
};
use axum::middleware::from_fn_with_state;
use axum::routing::{get, post};
use axum::Router;
use std::future::Future;
use std::net::SocketAddr;
use std::path::Path;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

/// The Content-Security-Policy served with the dashboard. Kept identical to the
/// one `frontend/vite.config.ts` injects at build time.
pub const DASHBOARD_CSP: &str = "default-src 'self'; connect-src 'self'; img-src 'self' data:; \
     style-src 'self' 'unsafe-inline'; script-src 'self'; base-uri 'none'; \
     form-action 'none'; frame-ancestors 'none'";

/// Serve the built dashboard at `/` with SPA fallback to `index.html`, plus the
/// CSP and a few hardening headers. Only the static responses carry these.
fn static_router(dir: &Path) -> Router<AppState> {
    let serve = ServeDir::new(dir).fallback(ServeFile::new(dir.join("index.html")));
    let hdr = |name: &'static str, val: &'static str| {
        SetResponseHeaderLayer::overriding(
            HeaderName::from_static(name),
            HeaderValue::from_static(val),
        )
    };
    Router::new()
        .fallback_service(serve)
        .layer(hdr("content-security-policy", DASHBOARD_CSP))
        .layer(hdr("x-content-type-options", "nosniff"))
        .layer(hdr("referrer-policy", "no-referrer"))
        .layer(hdr("x-frame-options", "DENY"))
}

fn build_cors(origins: &[String]) -> CorsLayer {
    if origins.is_empty() {
        // No configured origins → no CORS headers (same-origin only).
        return CorsLayer::new();
    }
    let allowed: Vec<HeaderValue> = origins.iter().filter_map(|o| o.parse().ok()).collect();
    CorsLayer::new()
        .allow_origin(allowed)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE])
}

/// Build the router. `/v1/health` and `/v1/metrics` are open; every other route
/// runs behind [`auth::require_auth`] and then checks its own minimum role.
/// Applied to everything, outermost first: rate limit → tracing → CORS →
/// metrics accounting.
pub fn router(state: AppState) -> Router {
    let cors = build_cors(&state.cors_origins);

    let protected = Router::new()
        .route("/v1/control", get(routes::get_control))
        .route("/v1/portfolio", get(routes::get_portfolio))
        .route("/v1/activity", get(routes::get_activity))
        .route("/v1/audit/verify", get(routes::get_audit_verify))
        .route("/v1/hook/pretooluse", post(routes::pretooluse))
        .route("/v1/mode", post(routes::post_mode))
        .route("/v1/kill", post(routes::post_kill))
        .route_layer(from_fn_with_state(state.clone(), auth::require_auth));

    let mut app = Router::new()
        .route("/v1/health", get(routes::health))
        .route("/v1/metrics", get(routes::metrics))
        .merge(protected);

    if let Some(dir) = &state.static_dir {
        // `/v1/*` above wins; anything else falls through to the dashboard.
        app = app.merge(static_router(dir));
    }

    app.layer(from_fn_with_state(state.clone(), mw::record_metrics))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .layer(from_fn_with_state(state.clone(), mw::rate_limit))
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
    use chrono::Utc;
    use http_body_util::BodyExt;
    use rust_decimal_macros::dec;
    use sherwood_core::{Asset, Fill, Portfolio, RiskConfig, RiskGate, Side, Venue};
    use sherwood_execution::{ToolAllowlist, ToolClass};
    use sherwood_store::{SqliteStore, Store};
    use std::sync::Arc;
    use tower::ServiceExt;

    const ADMIN: &str = "admin-token";
    const OPERATOR: &str = "operator-token";
    const VIEWER: &str = "viewer-token";

    fn state_full(opts: ServerOpts, store: Option<Arc<SqliteStore>>) -> AppState {
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
            Some(auth::ApiToken::from_value(VIEWER)),
        );
        AppState::new(tokens, risk, allowlist, opts, store)
    }

    fn state_with(opts: ServerOpts) -> AppState {
        state_full(opts, None)
    }

    fn test_state() -> AppState {
        state_with(ServerOpts::default())
    }

    /// An in-memory store holding one portfolio snapshot with an open position
    /// and one recorded fill, plus the audit rows that go with them.
    async fn seeded_store() -> Arc<SqliteStore> {
        let store = SqliteStore::open_in_memory().await.unwrap();
        let mut pf = Portfolio::new(dec!(1000));
        let fill = Fill {
            order_id: sherwood_core::OrderId::new("t-1"),
            asset: Asset::symbol("ROAR"),
            side: Side::Buy,
            qty: dec!(2),
            price: dec!(100),
            fee: dec!(0.1),
            venue: Venue::Paper,
            at: Utc::now(),
        };
        pf.apply(&fill);
        store.save_portfolio(&pf).await.unwrap();
        store.append_fill(&fill).await.unwrap();
        store
            .append_audit("order_fill", serde_json::json!({ "symbol": "ROAR" }))
            .await
            .unwrap();
        Arc::new(store)
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

    fn get(path: &str) -> Request<Body> {
        Request::get(path).body(Body::empty()).unwrap()
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
        let resp = call(test_state(), get("/v1/health")).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let b = body_string(resp).await;
        assert!(b.contains("\"mode\":\"paper\""));
        assert!(b.contains("\"kill_switch\":false"));
    }

    #[tokio::test]
    async fn metrics_is_open_and_counts_requests() {
        let state = test_state();
        call(state.clone(), get("/v1/health")).await;
        call(state.clone(), get("/v1/does-not-exist")).await;
        let resp = call(state, get("/v1/metrics")).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let b = body_string(resp).await;
        assert!(b.contains("sherwood_requests_total"));
        assert!(b.contains("sherwood_responses_total{class=\"2xx\"}"));
        assert!(b.contains("sherwood_responses_total{class=\"4xx\"}"));
        assert!(b.contains("sherwood_kill_switch 0"));
        assert!(b.contains("sherwood_mode_live 0"));
    }

    #[tokio::test]
    async fn rate_limit_returns_429_through_the_envelope() {
        let state = state_with(ServerOpts {
            rate_limit_per_min: 3,
            ..ServerOpts::default()
        });
        for _ in 0..3 {
            let r = call(state.clone(), get("/v1/health")).await;
            assert_eq!(r.status(), StatusCode::OK);
        }
        let resp = call(state, get("/v1/health")).await;
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(body_string(resp)
            .await
            .contains("\"code\":\"rate_limited\""));
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

        let allow = state_with(ServerOpts {
            allow_live: true,
            ..ServerOpts::default()
        });
        let resp = call(
            allow,
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
    async fn portfolio_and_activity_404_without_a_store() {
        for path in ["/v1/portfolio", "/v1/activity", "/v1/audit/verify"] {
            let mut req = get(path);
            req.headers_mut()
                .insert("authorization", format!("Bearer {VIEWER}").parse().unwrap());
            let resp = call(test_state(), req).await;
            assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{path}");
            assert!(body_string(resp).await.contains("\"code\":\"not_found\""));
        }
    }

    #[tokio::test]
    async fn portfolio_reads_the_persisted_snapshot() {
        let state = state_full(ServerOpts::default(), Some(seeded_store().await));
        let mut req = get("/v1/portfolio");
        req.headers_mut()
            .insert("authorization", format!("Bearer {VIEWER}").parse().unwrap());
        let resp = call(state, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let b = body_string(resp).await;
        assert!(b.contains("\"open_positions\":1"), "{b}");
        assert!(b.contains("\"symbol\":\"ROAR\""), "{b}");
    }

    #[tokio::test]
    async fn activity_and_audit_verify_read_the_store() {
        let store = seeded_store().await;
        let state = state_full(ServerOpts::default(), Some(store));

        let mut req = get("/v1/activity?limit=10");
        req.headers_mut()
            .insert("authorization", format!("Bearer {VIEWER}").parse().unwrap());
        let resp = call(state.clone(), req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(body_string(resp).await.contains("\"fills\":1"));

        let mut req = get("/v1/audit/verify");
        req.headers_mut()
            .insert("authorization", format!("Bearer {VIEWER}").parse().unwrap());
        let resp = call(state, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(body_string(resp).await.contains("\"ok\":true"));
    }

    #[tokio::test]
    async fn read_views_need_at_least_viewer() {
        let state = state_full(ServerOpts::default(), Some(seeded_store().await));
        let resp = call(state, get("/v1/portfolio")).await; // no token
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn serves_the_dashboard_with_spa_fallback_and_hardening_headers() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("index.html"),
            "<!doctype html><title>sherwood</title>",
        )
        .unwrap();
        let state = state_full(
            ServerOpts {
                static_dir: Some(dir.path().to_path_buf()),
                ..ServerOpts::default()
            },
            None,
        );

        // "/" serves index.html with the CSP + hardening headers.
        let resp = call(state.clone(), get("/")).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let csp = resp
            .headers()
            .get("content-security-policy")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(csp.contains("default-src 'self'") && csp.contains("script-src 'self'"));
        assert_eq!(resp.headers().get("x-frame-options").unwrap(), "DENY");
        assert_eq!(
            resp.headers().get("x-content-type-options").unwrap(),
            "nosniff"
        );
        assert!(body_string(resp).await.contains("<title>sherwood</title>"));

        // An unknown non-API path falls back to index.html (SPA routing).
        let resp = call(state.clone(), get("/portfolio")).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(body_string(resp).await.contains("sherwood"));

        // The API still takes precedence over the static fallback.
        let resp = call(state, get("/v1/health")).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(body_string(resp).await.contains("\"status\":\"ok\""));
    }

    #[tokio::test]
    async fn no_static_dir_means_unknown_paths_404() {
        let resp = call(test_state(), get("/not-a-route")).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
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
