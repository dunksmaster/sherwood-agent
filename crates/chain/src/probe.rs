//! The pre-flight from [ADR-0006](../../../docs/adr/0006-robinhood-chain-venue.md):
//! is a token's transfer permissionless at the contract, or is there an
//! allowlist / compliance hook?
//!
//! The method is a simulation, never a send: from a real funded holder, does
//! `eth_call transfer(<a fresh, un-onboarded address>, amount)` return `true`?
//! And does the *reverse* — a transfer *from* the fresh address — revert with a
//! plain `ERC20InsufficientBalance` (i.e. it failed only for lack of balance,
//! not identity)? Both true ⇒ permissionless.
//!
//! Live mode must run this and refuse to arm on anything but
//! [`Verdict::Permissionless`].

use crate::abi::{self, revert_selector};
use crate::erc20::{Erc20, TokenMeta};
use crate::keccak::{selector, topic_hex};
use crate::rpc::LogFilter;
use crate::{ChainError, EvmClient, Result};

/// EIP-1967 implementation slot.
const SLOT_IMPL: &str = "0x360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc";
/// EIP-1967 beacon slot.
const SLOT_BEACON: &str = "0xa3f0ad74e5423aebfd80d3ef4346578335a9a72aeaee59ff6cb3582b35133d50";
const ERR_INSUFFICIENT_BALANCE: &str = "0xe450d38c";
const ERR_INSUFFICIENT_ALLOWANCE: &str = "0xfb8f41b2";
const ZERO_ADDR: &str = "0x0000000000000000000000000000000000000000";
/// A deterministic address with no code and no history.
pub const FRESH_ADDRESS: &str = "0x1111111111111111111111111111111111111112";

/// How the token's logic is deployed — relevant because an upgradeable
/// implementation can gain a transfer allowlist later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyModel {
    /// Logic is in the account itself.
    None,
    /// EIP-1967 transparent/UUPS proxy.
    Eip1967 { implementation: String },
    /// Beacon proxy — one beacon controls many tokens' shared implementation.
    Beacon {
        beacon: String,
        implementation: Option<String>,
    },
}

/// The outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// A fresh address can receive; failures are balance-gated only.
    Permissionless,
    /// A transfer to a fresh address is blocked. Carries the revert selector or
    /// reason.
    Restricted(String),
    /// Could not complete the checks (no funded holder sampled, RPC gap, …).
    Inconclusive(String),
}

/// Everything the probe observed.
#[derive(Debug, Clone)]
pub struct ProbeReport {
    pub token: TokenMeta,
    pub total_supply_raw: u128,
    pub proxy: ProxyModel,
    pub paused: Option<bool>,
    pub sampled_blocks: u64,
    pub transfer_events: usize,
    pub wallet_to_wallet: usize,
    pub distinct_senders: usize,
    pub distinct_recipients: usize,
    pub holder_probed: Option<String>,
    pub transfer_to_fresh_ok: bool,
    pub reverse_is_balance_gated: bool,
    pub verdict: Verdict,
}

impl ProbeReport {
    /// `true` only for [`Verdict::Permissionless`].
    #[must_use]
    pub fn is_permissionless(&self) -> bool {
        self.verdict == Verdict::Permissionless
    }
}

/// Tunables. `Default` matches the JS reference probe.
#[derive(Debug, Clone)]
pub struct ProbeOptions {
    pub sample_blocks: u64,
    pub max_holders_scanned: usize,
    pub fresh_address: String,
}

impl Default for ProbeOptions {
    fn default() -> Self {
        Self {
            sample_blocks: 1500,
            max_holders_scanned: 25,
            fresh_address: FRESH_ADDRESS.to_owned(),
        }
    }
}

/// Run the pre-flight against `token_address`.
pub async fn check_transfer_open<C: EvmClient + ?Sized>(
    client: &C,
    token_address: &str,
    opts: &ProbeOptions,
) -> Result<ProbeReport> {
    let token = Erc20::new(client, token_address)?;
    let meta = token.meta().await?;
    let total_supply_raw = token.total_supply_raw().await.unwrap_or(0);
    let proxy = detect_proxy(client, token_address).await?;
    let paused = read_paused(client, token_address).await;

    // ---- recent Transfer traffic --------------------------------------------
    let head = client.block_number().await?;
    let from_block = head.saturating_sub(opts.sample_blocks);
    let filter = LogFilter {
        address: Some(token_address.to_lowercase()),
        topics: vec![Some(topic_hex("Transfer(address,address,uint256)"))],
        from_block,
        to_block: head,
    };
    let logs = client.get_logs(&filter).await?;

    let mut senders = std::collections::BTreeSet::new();
    let mut recipients = std::collections::BTreeSet::new();
    let mut wallet_to_wallet = 0usize;
    for l in &logs {
        let (Some(f), Some(t)) = (l.topic_address(1), l.topic_address(2)) else {
            continue;
        };
        if f != ZERO_ADDR && t != ZERO_ADDR {
            wallet_to_wallet += 1;
        }
        senders.insert(f);
        recipients.insert(t);
    }

    // ---- the test ----------------------------------------------------------
    let holder = pick_holder(&token, &senders, opts.max_holders_scanned).await;
    let (holder_probed, transfer_to_fresh_ok, reverse_is_balance_gated, verdict) = match holder {
        None => (
            None,
            false,
            false,
            Verdict::Inconclusive("no funded holder in the sampled window".into()),
        ),
        Some(h) => {
            let to_fresh =
                simulate_transfer(client, token_address, &h, &opts.fresh_address, 1000).await;
            let from_fresh =
                simulate_transfer(client, token_address, &opts.fresh_address, &h, 1).await;

            let ok = matches!(&to_fresh, SimResult::Ok);
            let balance_gated = matches!(&from_fresh, SimResult::RevertSelector(s)
                if s == ERR_INSUFFICIENT_BALANCE || s == ERR_INSUFFICIENT_ALLOWANCE);

            let verdict = match &to_fresh {
                SimResult::Ok if balance_gated => Verdict::Permissionless,
                SimResult::Ok => Verdict::Inconclusive(
                    "transfer to a fresh address succeeded but the reverse did not fail cleanly \
                     for balance — re-run"
                        .into(),
                ),
                SimResult::RevertSelector(s) => {
                    Verdict::Restricted(format!("transfer to a fresh address reverts ({s})"))
                }
                SimResult::RevertReason(r) => {
                    Verdict::Restricted(format!("transfer to a fresh address reverts: {r}"))
                }
                SimResult::Err(e) => Verdict::Inconclusive(e.clone()),
            };
            (Some(h), ok, balance_gated, verdict)
        }
    };

    Ok(ProbeReport {
        token: meta,
        total_supply_raw,
        proxy,
        paused,
        sampled_blocks: head - from_block,
        transfer_events: logs.len(),
        wallet_to_wallet,
        distinct_senders: senders.len(),
        distinct_recipients: recipients.len(),
        holder_probed,
        transfer_to_fresh_ok,
        reverse_is_balance_gated,
        verdict,
    })
}

enum SimResult {
    Ok,
    RevertSelector(String),
    RevertReason(String),
    Err(String),
}

async fn simulate_transfer<C: EvmClient + ?Sized>(
    client: &C,
    token: &str,
    from: &str,
    to: &str,
    amount: u128,
) -> SimResult {
    let data = match (abi::address_word(to), abi::address_word(from)) {
        (Ok(to_word), Ok(_)) => abi::calldata(
            selector("transfer(address,uint256)"),
            &[to_word, abi::uint_word(amount)],
        ),
        _ => return SimResult::Err("bad address".into()),
    };
    match client.call_from(from, token, &data).await {
        Ok(ret) => {
            // ERC-20 `transfer` returns bool; treat empty or `true` as success
            // (some tokens return nothing), a `false` word as a soft failure.
            if ret.is_empty() || abi::decode_u128(&ret).map(|v| v == 1).unwrap_or(false) {
                SimResult::Ok
            } else {
                SimResult::RevertReason("transfer() returned false".into())
            }
        }
        Err(ChainError::Reverted {
            reason: Some(r), ..
        }) => SimResult::RevertReason(r),
        Err(ChainError::Reverted { data, .. }) => SimResult::RevertSelector(
            revert_selector(data.as_deref()).unwrap_or_else(|| "0x".into()),
        ),
        Err(e) => SimResult::Err(e.to_string()),
    }
}

async fn pick_holder<C: EvmClient + ?Sized>(
    token: &Erc20<'_, C>,
    senders: &std::collections::BTreeSet<String>,
    max_scan: usize,
) -> Option<String> {
    let mut best: Option<(String, u128)> = None;
    for a in senders.iter().filter(|a| *a != ZERO_ADDR).take(max_scan) {
        if let Ok(bal) = token.balance_of_raw(a).await {
            let improves = match &best {
                None => true,
                Some((_, b)) => bal > *b,
            };
            if bal > 0 && improves {
                best = Some((a.clone(), bal));
            }
        }
    }
    best.map(|(a, _)| a)
}

async fn detect_proxy<C: EvmClient + ?Sized>(client: &C, token: &str) -> Result<ProxyModel> {
    let impl_slot = client.get_storage_at(token, SLOT_IMPL).await?;
    if let Some(a) = nonzero_address(&impl_slot) {
        return Ok(ProxyModel::Eip1967 { implementation: a });
    }
    let beacon_slot = client.get_storage_at(token, SLOT_BEACON).await?;
    if let Some(beacon) = nonzero_address(&beacon_slot) {
        let implementation = client
            .call(&beacon, &abi::calldata(selector("implementation()"), &[]))
            .await
            .ok()
            .and_then(|r| abi::decode_address(&r).ok())
            .filter(|a| a != ZERO_ADDR);
        return Ok(ProxyModel::Beacon {
            beacon,
            implementation,
        });
    }
    Ok(ProxyModel::None)
}

async fn read_paused<C: EvmClient + ?Sized>(client: &C, token: &str) -> Option<bool> {
    let r = client
        .call(token, &abi::calldata(selector("paused()"), &[]))
        .await
        .ok()?;
    abi::decode_u128(&r).ok().map(|v| v != 0)
}

fn nonzero_address(word: &[u8; 32]) -> Option<String> {
    let low = &word[12..32];
    low.iter().any(|&b| b != 0).then(|| abi::to_hex(low))
}

impl std::fmt::Display for ProbeReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "{} ({})  {}",
            self.token.symbol, self.token.name, self.token.address
        )?;
        writeln!(f, "  decimals            {}", self.token.decimals)?;
        match &self.proxy {
            ProxyModel::None => writeln!(f, "  proxy               none (immutable logic)")?,
            ProxyModel::Eip1967 { implementation } => {
                writeln!(f, "  proxy               EIP-1967 → {implementation}")?;
            }
            ProxyModel::Beacon {
                beacon,
                implementation,
            } => {
                writeln!(f, "  proxy               beacon {beacon}")?;
                writeln!(
                    f,
                    "                      → impl {}  (shared; upgradeable)",
                    implementation.as_deref().unwrap_or("?")
                )?;
            }
        }
        if let Some(p) = self.paused {
            writeln!(f, "  paused()            {p}")?;
        }
        writeln!(
            f,
            "  {} blk sampled    {} Transfer events, {} wallet↔wallet, {} senders / {} recipients",
            self.sampled_blocks,
            self.transfer_events,
            self.wallet_to_wallet,
            self.distinct_senders,
            self.distinct_recipients
        )?;
        if let Some(h) = &self.holder_probed {
            writeln!(f, "  holder probed       {h}")?;
        }
        writeln!(
            f,
            "  transfer → fresh    {}",
            if self.transfer_to_fresh_ok {
                "ok (returns true)"
            } else {
                "NOT ok"
            }
        )?;
        writeln!(
            f,
            "  transfer ← fresh    {}",
            if self.reverse_is_balance_gated {
                "reverts ERC20InsufficientBalance (balance-gated only)"
            } else {
                "did not fail cleanly for balance"
            }
        )?;
        match &self.verdict {
            Verdict::Permissionless => write!(f, "  VERDICT             permissionless ✅"),
            Verdict::Restricted(why) => write!(f, "  VERDICT             RESTRICTED ⛔  {why}"),
            Verdict::Inconclusive(why) => write!(f, "  VERDICT             inconclusive — {why}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::MockRpc;
    use serde_json::json;

    fn w(hex: &str) -> serde_json::Value {
        json!(format!("0x{:0>64}", hex))
    }
    fn transfer_log(from_suffix: &str, to_suffix: &str) -> serde_json::Value {
        json!({
            "address": "0xtoken",
            "topics": [
                topic_hex("Transfer(address,address,uint256)"),
                format!("0x{:0>64}", from_suffix),
                format!("0x{:0>64}", to_suffix),
            ],
            "data": "0x00",
            "blockNumber": "0x10"
        })
    }

    // name, symbol, decimals, totalSupply, impl slot, beacon slot,
    // beacon.implementation(), paused(), blockNumber, getLogs, balanceOf(holder),
    // transfer→fresh, transfer←fresh
    fn happy_replies() -> Vec<Result<serde_json::Value>> {
        vec![
            Ok(json!(
                "0x0000000000000000000000000000000000000000000000000000000000000020\
                       0000000000000000000000000000000000000000000000000000000000000004\
                       4e56444100000000000000000000000000000000000000000000000000000000"
            )), // name
            Ok(json!(
                "0x0000000000000000000000000000000000000000000000000000000000000020\
                       0000000000000000000000000000000000000000000000000000000000000004\
                       4e56444100000000000000000000000000000000000000000000000000000000"
            )), // symbol
            Ok(w("12")),                                       // decimals = 18
            Ok(w("d3c21bcecceda1000000")),                     // totalSupply
            Ok(w("0")),                                        // EIP-1967 impl slot: empty
            Ok(w("e10b6f6b275de231345c20d14ab812db62151b00")), // beacon slot
            Ok(w("b35490d6f9163de4f80d88dc75c3516eb64c5ae2")), // beacon.implementation()
            Ok(w("0")),                                        // paused() = false
            Ok(json!("0x10")),                                 // blockNumber = 16
            Ok(json!([
                transfer_log("aaaa", "bbbb"),
                transfer_log("aaaa", "cccc"),
            ])), // getLogs — one distinct sender
            Ok(w("14d1120d7b160000")),                         // balanceOf(holder) = 1.5e18
            Ok(w("1")),                                        // transfer→fresh returns true
            Err(ChainError::Reverted {
                reason: None,
                data: Some(format!("{ERR_INSUFFICIENT_BALANCE}00")),
            }), // transfer←fresh reverts InsufficientBalance
        ]
    }

    #[tokio::test]
    async fn happy_path_is_permissionless() {
        let rpc = MockRpc::new(happy_replies());
        let r = check_transfer_open(
            &rpc,
            "0xd0601ce157db5bdc3162bbac2a2c8af5320d9eec",
            &ProbeOptions::default(),
        )
        .await
        .unwrap();
        assert_eq!(r.token.symbol, "NVDA");
        assert!(matches!(r.proxy, ProxyModel::Beacon { .. }));
        assert_eq!(r.paused, Some(false));
        assert!(r.transfer_to_fresh_ok);
        assert!(r.reverse_is_balance_gated);
        assert_eq!(r.verdict, Verdict::Permissionless);
        assert!(r.is_permissionless());
    }

    #[tokio::test]
    async fn a_reverting_transfer_to_fresh_is_restricted() {
        let mut replies = happy_replies();
        // replace the transfer→fresh reply (index 11) with an allowlist-style revert
        replies[11] = Err(ChainError::Reverted {
            reason: Some("HOOD: recipient not allowed".into()),
            data: None,
        });
        let rpc = MockRpc::new(replies);
        let r = check_transfer_open(
            &rpc,
            "0xd0601ce157db5bdc3162bbac2a2c8af5320d9eec",
            &ProbeOptions::default(),
        )
        .await
        .unwrap();
        assert!(!r.transfer_to_fresh_ok);
        assert!(matches!(r.verdict, Verdict::Restricted(_)));
        assert!(!r.is_permissionless());
    }

    #[tokio::test]
    async fn no_holder_sampled_is_inconclusive() {
        let mut replies = happy_replies();
        replies[9] = Ok(json!([])); // getLogs: nothing
        replies.truncate(10); // no balanceOf / transfer calls will be made
        let rpc = MockRpc::new(replies);
        let r = check_transfer_open(
            &rpc,
            "0xd0601ce157db5bdc3162bbac2a2c8af5320d9eec",
            &ProbeOptions::default(),
        )
        .await
        .unwrap();
        assert!(matches!(r.verdict, Verdict::Inconclusive(_)));
    }
}
