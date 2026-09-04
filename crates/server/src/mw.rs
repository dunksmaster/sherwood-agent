//! Cross-cutting middleware: the global rate limit and request accounting.

use axum::extract::State;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::ApiError;
use crate::state::AppState;

/// Reject with `429` when the global per-minute window is full.
pub async fn rate_limit(
    State(state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, ApiError> {
    if state.limiter.check() {
        Ok(next.run(request).await)
    } else {
        Err(ApiError::TooManyRequests)
    }
}

/// Count every response by status class after the handler runs.
pub async fn record_metrics(
    State(state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let resp = next.run(request).await;
    state.metrics.record(resp.status().as_u16());
    resp.into_response()
}
