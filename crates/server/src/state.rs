//! Shared, cheaply-cloneable handler state.

use crate::auth::ApiToken;
use chrono::{DateTime, Utc};
use sherwood_core::RiskGate;
use sherwood_execution::ToolAllowlist;
use std::sync::Arc;

/// Trading mode. Only [`Mode::Paper`] is reachable in v0.1; the LIVE toggle and
/// its `admin` + re-auth gate arrive with S9b.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Paper,
    Live,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Live => "live",
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub token: Arc<ApiToken>,
    /// The risk config the `PreToolUse` hook checks orders against. `Arc` for
    /// now; S9b makes this swappable for the LIVE toggle and kill switch.
    pub risk: Arc<RiskGate>,
    /// Which agent MCP tools may be called, and how each is classified.
    pub allowlist: Arc<ToolAllowlist>,
    pub mode: Mode,
    pub started_at: DateTime<Utc>,
}

impl AppState {
    pub fn new(token: ApiToken, risk: RiskGate, allowlist: ToolAllowlist) -> Self {
        Self {
            token: Arc::new(token),
            risk: Arc::new(risk),
            allowlist: Arc::new(allowlist),
            mode: Mode::Paper,
            started_at: Utc::now(),
        }
    }

    pub fn uptime_secs(&self) -> i64 {
        (Utc::now() - self.started_at).num_seconds().max(0)
    }
}
