//! Wires [ADR-0006](../../../docs/adr/0006-robinhood-chain-venue.md)'s
//! mandatory pre-flight into `sherwood-server`'s `LivePreflight` trait
//! (v0.2.6): `POST /v1/mode` calls this, on every attempt to arm `Live`,
//! before the mode flag flips.
//!
//! Runs `sherwood_chain::probe::check_transfer_open` — the same check
//! `sherwood chain-probe` runs by hand — against every symbol in `[chain]
//! symbols` plus the configured `denom` (it moves too, via `SETTLE_ALL`).
//! Any token that is not `Verdict::Permissionless`, or an RPC/connection
//! failure, refuses to arm. No symbols configured also refuses: there is no
//! declared live-trade universe to have checked.

use async_trait::async_trait;
use sherwood_chain::tokens;
use sherwood_chain::{probe, HttpClient};
use sherwood_server::state::LivePreflight;
use std::time::Duration;

pub struct ChainLivePreflight {
    pub rpc_url: String,
    /// Known symbols or raw addresses to check. Built from `[chain] symbols`
    /// plus `[chain] denom`, deduplicated.
    pub tokens: Vec<String>,
    pub timeout: Duration,
}

#[async_trait]
impl LivePreflight for ChainLivePreflight {
    async fn check(&self) -> Result<(), String> {
        if self.tokens.is_empty() {
            return Err(
                "no [chain] symbols configured to pre-flight — nothing to have checked".into(),
            );
        }

        let client = HttpClient::new(self.rpc_url.clone(), self.timeout)
            .map_err(|e| format!("connecting to {}: {e}", self.rpc_url))?;

        for raw in &self.tokens {
            let (symbol, address, _decimals) = tokens::resolve(raw);
            let report =
                probe::check_transfer_open(&client, &address, &probe::ProbeOptions::default())
                    .await
                    .map_err(|e| format!("{symbol}: pre-flight RPC error: {e}"))?;
            match report.verdict {
                probe::Verdict::Permissionless => {}
                probe::Verdict::Restricted(why) => {
                    return Err(format!("{symbol} ({address}) is RESTRICTED: {why}"));
                }
                probe::Verdict::Inconclusive(why) => {
                    return Err(format!(
                        "{symbol} ({address}) pre-flight was inconclusive: {why}"
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Build the token list from `[chain]`: every `symbols` entry plus `denom`,
/// deduplicated case-insensitively.
#[must_use]
pub fn preflight_tokens(symbols: &[String], denom: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(symbols.len() + 1);
    for s in symbols.iter().chain(std::iter::once(&denom.to_owned())) {
        if !out
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(s))
        {
            out.push(s.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflight_tokens_dedupes_case_insensitively_and_includes_denom() {
        let out = preflight_tokens(&["NVDA".into(), "usdg".into(), "TSLA".into()], "USDG");
        assert_eq!(
            out,
            vec!["NVDA".to_owned(), "usdg".to_owned(), "TSLA".to_owned()]
        );
    }

    #[test]
    fn preflight_tokens_adds_denom_when_absent() {
        let out = preflight_tokens(&["NVDA".into()], "USDG");
        assert_eq!(out, vec!["NVDA".to_owned(), "USDG".to_owned()]);
    }

    #[test]
    fn preflight_tokens_empty_symbols_still_has_the_denom() {
        let out = preflight_tokens(&[], "USDG");
        assert_eq!(out, vec!["USDG".to_owned()]);
    }
}
