//! Bearer-token auth for the local API.
//!
//! One token, generated on first run and stored in the `sherwood-secrets`
//! vault. Presented as `Authorization: Bearer <token>` and compared in
//! constant time. There is no login, no session, no refresh — this is a
//! single-operator control plane bound to loopback.
//!
//! RBAC roles (`viewer` / `operator` / `admin`) arrive with S9b; today every
//! valid token is fully privileged.

use axum::extract::State;
use axum::http::{header::AUTHORIZATION, Request};
use axum::middleware::Next;
use axum::response::Response;
use sherwood_secrets::{SecretString, SecretsVault};
use subtle::ConstantTimeEq;

use crate::error::ApiError;
use crate::state::AppState;

/// The API token. Wraps a [`SecretString`] so it is zeroized on drop and never
/// appears in a `Debug` log.
#[derive(Clone)]
pub struct ApiToken(SecretString);

/// Whether [`ApiToken::load_or_create`] had to mint a new token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenOrigin {
    Loaded,
    Created,
}

impl ApiToken {
    /// 32 bytes of CSPRNG output, hex-encoded (64 chars).
    fn generate() -> Result<String, ApiError> {
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes)
            .map_err(|e| ApiError::internal(format!("CSPRNG unavailable: {e}")))?;
        Ok(hex::encode(bytes))
    }

    /// Read the token named `name` from `vault`; generate and store one if it
    /// is not there yet.
    pub fn load_or_create(
        vault: &dyn SecretsVault,
        name: &str,
    ) -> Result<(Self, TokenOrigin), ApiError> {
        if let Some(existing) = vault
            .get(name)
            .map_err(|e| ApiError::internal(format!("vault read failed: {e}")))?
        {
            return Ok((Self(existing), TokenOrigin::Loaded));
        }
        let fresh = Self::generate()?;
        vault
            .set(name, &fresh)
            .map_err(|e| ApiError::internal(format!("vault write failed: {e}")))?;
        Ok((Self(SecretString::new(&fresh)), TokenOrigin::Created))
    }

    /// Build directly from a known value (tests, or a token supplied out of
    /// band).
    pub fn from_value(value: impl Into<String>) -> Self {
        Self(SecretString::new(value.into()))
    }

    /// Constant-time equality against a presented token. Length differences are
    /// not short-circuited into an early return that a timing attack could read.
    pub fn verify(&self, presented: &str) -> bool {
        let expected = self.0.expose().as_bytes();
        let got = presented.as_bytes();
        if expected.len() != got.len() {
            // Still do a comparison so the fast path and the slow path take a
            // similar amount of time; the result is forced to false.
            let _ = expected.ct_eq(&got[..got.len().min(expected.len())]);
            return false;
        }
        expected.ct_eq(got).into()
    }
}

impl std::fmt::Debug for ApiToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiToken([redacted])")
    }
}

/// Axum middleware: require a valid `Authorization: Bearer <token>`.
pub async fn require_token(
    State(state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let presented = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .ok_or(ApiError::Unauthorized)?;

    if !state.token.verify(presented) {
        return Err(ApiError::Unauthorized);
    }
    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_accepts_the_exact_token_and_rejects_others() {
        let t = ApiToken::from_value("s3cr3t-token-value");
        assert!(t.verify("s3cr3t-token-value"));
        assert!(!t.verify("s3cr3t-token-valuE"));
        assert!(!t.verify("s3cr3t-token-value-longer"));
        assert!(!t.verify("short"));
        assert!(!t.verify(""));
    }

    #[test]
    fn generate_is_64_hex_chars_and_unique() {
        let a = ApiToken::generate().unwrap();
        let b = ApiToken::generate().unwrap();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn debug_is_redacted() {
        let t = ApiToken::from_value("do-not-print-me");
        assert_eq!(format!("{t:?}"), "ApiToken([redacted])");
    }
}
