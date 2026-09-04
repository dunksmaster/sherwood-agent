//! The one error envelope, per
//! [ENGINEERING-STANDARDS.md](../../../docs/ENGINEERING-STANDARDS.md#api):
//! `{ code, message, correlation_id }`, the same shape on every failing route.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

/// A request-scoped identifier echoed back in every response so a client log
/// line and a server log line can be tied together.
pub type CorrelationId = String;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("missing or malformed Authorization header")]
    Unauthorized,
    /// Authenticated, but the role is insufficient (or re-auth failed).
    #[error("{0}")]
    Forbidden(String),
    /// The request was syntactically wrong (bad JSON, missing field).
    #[error("{0}")]
    BadRequest(String),
    /// The addressed resource does not exist (or is not configured).
    #[error("{0}")]
    NotFound(String),
    /// The request parsed but described something we cannot act on.
    #[error("{0}")]
    Unprocessable(String),
    /// The global rate-limit window is full.
    #[error("rate limit exceeded — slow down")]
    TooManyRequests,
    /// Our fault. The message is logged, never returned.
    #[error("internal error")]
    Internal(String),
}

impl ApiError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::Forbidden(msg.into())
    }

    pub fn unprocessable(msg: impl Into<String>) -> Self {
        Self::Unprocessable(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    fn status(&self) -> StatusCode {
        match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Unprocessable(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::BadRequest(_) => "bad_request",
            Self::NotFound(_) => "not_found",
            Self::Unprocessable(_) => "unprocessable_entity",
            Self::TooManyRequests => "rate_limited",
            Self::Internal(_) => "internal",
        }
    }
}

#[derive(Serialize)]
struct Envelope<'a> {
    code: &'a str,
    message: String,
    correlation_id: CorrelationId,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let correlation_id = crate::new_correlation_id();
        let status = self.status();
        match &self {
            Self::Internal(detail) => {
                tracing::error!(%correlation_id, detail, "request failed");
            }
            _ => tracing::debug!(%correlation_id, %status, error = %self, "request rejected"),
        }
        let body = Envelope {
            code: self.code(),
            // Internal errors never leak their detail to the client.
            message: match &self {
                Self::Internal(_) => "internal error".to_string(),
                other => other.to_string(),
            },
            correlation_id,
        };
        (status, Json(body)).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
