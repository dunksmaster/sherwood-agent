//! Bearer-token auth and RBAC for the local API.
//!
//! Tokens are generated on first run and stored in the `sherwood-secrets`
//! vault, presented as `Authorization: Bearer <token>`, and compared in
//! constant time. There is no login, no session, no refresh — this is a
//! control plane bound to loopback.
//!
//! RBAC has three roles. v0.1 typically runs with only the admin token
//! configured; adding operator / viewer tokens later is a config change, not a
//! redesign — every route already declares the role it needs.
//!
//! * `viewer` — read state.
//! * `operator` — the above, plus the `PreToolUse` order gate.
//! * `admin` — the above, plus the mode toggle and the kill switch (each also
//!   requires the admin token again in the request body).

use axum::extract::{FromRequestParts, State};
use axum::http::{header::AUTHORIZATION, request::Parts, Request};
use axum::middleware::Next;
use axum::response::Response;
use serde::Serialize;
use sherwood_secrets::{SecretString, SecretsVault};
use subtle::ConstantTimeEq;

use crate::error::ApiError;
use crate::state::AppState;

/// Access level, ordered: `Viewer < Operator < Admin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Viewer,
    Operator,
    Admin,
}

/// One bearer token. Wraps a [`SecretString`] so it is zeroised on drop and
/// never appears in a `Debug` log.
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

    /// Constant-time equality against a presented token.
    pub fn verify(&self, presented: &str) -> bool {
        let expected = self.0.expose().as_bytes();
        let got = presented.as_bytes();
        if expected.len() != got.len() {
            // Touch a comparison of the same length so the two paths cost
            // roughly the same, then force the result false.
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

/// The configured tokens, one per role. Only `admin` is required.
pub struct TokenSet {
    admin: ApiToken,
    operator: Option<ApiToken>,
    viewer: Option<ApiToken>,
}

impl TokenSet {
    pub fn new(admin: ApiToken, operator: Option<ApiToken>, viewer: Option<ApiToken>) -> Self {
        Self {
            admin,
            operator,
            viewer,
        }
    }

    /// Admin-only set (the common v0.1 shape).
    pub fn admin_only(admin: ApiToken) -> Self {
        Self::new(admin, None, None)
    }

    /// The highest role a presented token grants, or `None` if it matches no
    /// configured token. Checked most-privileged first so a token reused across
    /// slots still authenticates at its highest level.
    pub fn role_for(&self, presented: &str) -> Option<Role> {
        if self.admin.verify(presented) {
            return Some(Role::Admin);
        }
        if self.operator.as_ref().is_some_and(|t| t.verify(presented)) {
            return Some(Role::Operator);
        }
        if self.viewer.as_ref().is_some_and(|t| t.verify(presented)) {
            return Some(Role::Viewer);
        }
        None
    }

    /// True if `presented` is the admin token — used to re-authorise a
    /// privileged action from the request body.
    pub fn is_admin(&self, presented: &str) -> bool {
        self.admin.verify(presented)
    }
}

fn bearer(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
}

/// Axum middleware: require a valid bearer token and stash its [`Role`] in the
/// request extensions for [`Caller`] to pick up.
pub async fn require_auth(
    State(state): State<AppState>,
    mut request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let presented = bearer(request.headers()).ok_or(ApiError::Unauthorized)?;
    let role = state
        .tokens
        .role_for(presented)
        .ok_or(ApiError::Unauthorized)?;
    request.extensions_mut().insert(role);
    Ok(next.run(request).await)
}

/// Extractor for the authenticated caller's role. Requires [`require_auth`] to
/// have run first.
#[derive(Debug, Clone, Copy)]
pub struct Caller(pub Role);

impl Caller {
    /// `Ok` if the caller has at least `needed`, else [`ApiError::Forbidden`].
    pub fn require(&self, needed: Role) -> Result<(), ApiError> {
        if self.0 >= needed {
            Ok(())
        } else {
            Err(ApiError::Forbidden(format!(
                "this action needs the `{needed:?}` role"
            )))
        }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, ApiError> {
        parts
            .extensions
            .get::<Role>()
            .copied()
            .map(Caller)
            .ok_or(ApiError::Unauthorized)
    }
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

    #[test]
    fn role_for_picks_the_matching_slot() {
        let set = TokenSet::new(
            ApiToken::from_value("admin-tok"),
            Some(ApiToken::from_value("op-tok")),
            Some(ApiToken::from_value("view-tok")),
        );
        assert_eq!(set.role_for("admin-tok"), Some(Role::Admin));
        assert_eq!(set.role_for("op-tok"), Some(Role::Operator));
        assert_eq!(set.role_for("view-tok"), Some(Role::Viewer));
        assert_eq!(set.role_for("nope"), None);
        assert!(set.is_admin("admin-tok"));
        assert!(!set.is_admin("op-tok"));
    }

    #[test]
    fn admin_only_set_rejects_absent_roles() {
        let set = TokenSet::admin_only(ApiToken::from_value("only-admin"));
        assert_eq!(set.role_for("only-admin"), Some(Role::Admin));
        assert_eq!(set.role_for("anything-else"), None);
    }

    #[test]
    fn role_ordering_is_viewer_lt_operator_lt_admin() {
        assert!(Role::Viewer < Role::Operator && Role::Operator < Role::Admin);
        assert!(Caller(Role::Admin).require(Role::Operator).is_ok());
        assert!(Caller(Role::Viewer).require(Role::Operator).is_err());
    }
}
