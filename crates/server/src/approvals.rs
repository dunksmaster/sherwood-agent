//! The approval gate — a human-in-the-loop step between "the risk gate would
//! allow this order" and "the agent may place it".
//!
//! In `auto` mode the gate is transparent: a risk-passing order is allowed
//! immediately, as before. In `manual` mode every risk-passing order becomes a
//! **pending approval**; the `PreToolUse` hook holds its response until the
//! operator approves or denies it, or an auto-deny timeout fires.
//!
//! v0.1 models `pending → approved | denied | expired`. `executed` and
//! `settled` are deliberately absent: they require observing the fill against
//! Robinhood's ledger (S7.4), which is not wired until the live MCP exists.
//! See [ADR-0005](../../../docs/adr/0005-approval-gate.md).

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use sherwood_core::{Order, Side};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalState {
    Pending,
    Approved,
    Denied,
    Expired,
}

impl ApprovalState {
    fn is_terminal(self) -> bool {
        !matches!(self, Self::Pending)
    }
}

/// The parts of an [`Order`] shown on the approval card.
#[derive(Debug, Clone, Serialize)]
pub struct OrderSummary {
    pub symbol: String,
    pub side: &'static str,
    pub quantity: Decimal,
    pub limit_price: Option<Decimal>,
    /// Free-form provenance from the order (`agent tool call: place_order`).
    pub reason: String,
}

impl From<&Order> for OrderSummary {
    fn from(o: &Order) -> Self {
        Self {
            symbol: o.asset.symbol.clone(),
            side: match o.side {
                Side::Buy => "buy",
                Side::Sell => "sell",
            },
            quantity: o.qty,
            limit_price: o.limit_price,
            reason: o.reason.clone(),
        }
    }
}

/// One approval, as returned by `GET /v1/approvals`.
#[derive(Debug, Clone, Serialize)]
pub struct Approval {
    pub id: String,
    pub state: ApprovalState,
    pub tool: String,
    pub order: OrderSummary,
    pub created_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
    /// Operator's note on a decision, if any.
    pub decision_reason: Option<String>,
}

struct Entry {
    approval: Approval,
    tx: watch::Sender<ApprovalState>,
}

/// In-memory queue of approvals. Decisions are delivered to a waiting hook
/// through a per-approval [`watch`] channel. Terminal entries are retained
/// (capped) so the dashboard can show recent history.
pub struct ApprovalStore {
    inner: Mutex<Inner>,
    timeout: Duration,
    retain: usize,
}

struct Inner {
    entries: Vec<Entry>,
    seq: u64,
}

/// Handle a waiting hook uses to await the operator's decision.
pub struct Ticket {
    pub id: String,
    rx: watch::Receiver<ApprovalState>,
    timeout: Duration,
}

impl Ticket {
    /// Block until the approval leaves `Pending`, or the timeout fires. A
    /// timeout resolves to [`ApprovalState::Expired`]; the store's sweeper
    /// records the same so the two never disagree for long.
    pub async fn wait(mut self) -> ApprovalState {
        match tokio::time::timeout(self.timeout, async {
            // `changed()` returns once per send; loop until terminal.
            loop {
                if self.rx.borrow().is_terminal() {
                    return *self.rx.borrow();
                }
                if self.rx.changed().await.is_err() {
                    return ApprovalState::Expired; // store dropped
                }
            }
        })
        .await
        {
            Ok(state) => state,
            Err(_) => ApprovalState::Expired,
        }
    }
}

impl ApprovalStore {
    pub fn new(timeout: Duration) -> Self {
        Self {
            inner: Mutex::new(Inner {
                entries: Vec::new(),
                seq: 0,
            }),
            timeout,
            retain: 50,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Enqueue a pending approval for `order` and return a [`Ticket`] the hook
    /// awaits.
    pub fn enqueue(&self, tool: &str, order: &Order) -> Ticket {
        let mut inner = self.lock();
        inner.seq += 1;
        let id = format!("apr-{}-{}", Utc::now().timestamp_millis(), inner.seq);
        let (tx, rx) = watch::channel(ApprovalState::Pending);
        inner.entries.push(Entry {
            approval: Approval {
                id: id.clone(),
                state: ApprovalState::Pending,
                tool: tool.to_string(),
                order: OrderSummary::from(order),
                created_at: Utc::now(),
                decided_at: None,
                decision_reason: None,
            },
            tx,
        });
        Ticket {
            id,
            rx,
            timeout: self.timeout,
        }
    }

    /// Resolve `id` to `state` (must be terminal). `Ok(false)` if the id is
    /// unknown or already decided.
    pub fn decide(
        &self,
        id: &str,
        state: ApprovalState,
        reason: Option<String>,
    ) -> Result<bool, &'static str> {
        if !state.is_terminal() {
            return Err("decision state must be terminal");
        }
        let mut inner = self.lock();
        let Some(entry) = inner.entries.iter_mut().find(|e| e.approval.id == id) else {
            return Ok(false);
        };
        if entry.approval.state.is_terminal() {
            return Ok(false);
        }
        entry.approval.state = state;
        entry.approval.decided_at = Some(Utc::now());
        entry.approval.decision_reason = reason;
        let _ = entry.tx.send(state);
        Self::trim(&mut inner, self.retain);
        Ok(true)
    }

    /// Mark every approval older than the timeout as `Expired`. Called on a
    /// timer by the sweeper spawned in `serve`.
    pub fn sweep_expired(&self) {
        let now = Utc::now();
        let mut inner = self.lock();
        let cutoff = chrono::Duration::from_std(self.timeout).unwrap_or(chrono::Duration::zero());
        for e in inner.entries.iter_mut() {
            if e.approval.state == ApprovalState::Pending && now - e.approval.created_at > cutoff {
                e.approval.state = ApprovalState::Expired;
                e.approval.decided_at = Some(now);
                let _ = e.tx.send(ApprovalState::Expired);
            }
        }
        let retain = self.retain;
        Self::trim(&mut inner, retain);
    }

    /// Newest first: every pending approval, then recently decided ones.
    pub fn list(&self) -> Vec<Approval> {
        let inner = self.lock();
        let mut out: Vec<Approval> = inner.entries.iter().map(|e| e.approval.clone()).collect();
        out.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        out
    }

    pub fn pending_count(&self) -> usize {
        self.lock()
            .entries
            .iter()
            .filter(|e| e.approval.state == ApprovalState::Pending)
            .count()
    }

    fn trim(inner: &mut Inner, retain: usize) {
        let decided: Vec<usize> = inner
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.approval.state.is_terminal())
            .map(|(i, _)| i)
            .collect();
        if decided.len() > retain {
            let drop: std::collections::HashSet<usize> =
                decided[..decided.len() - retain].iter().copied().collect();
            let mut i = 0;
            inner.entries.retain(|_| {
                let keep = !drop.contains(&i);
                i += 1;
                keep
            });
        }
    }
}

/// `auto` = the gate is transparent (risk gate decides). `manual` = every
/// risk-passing order waits for the operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalMode {
    Auto,
    Manual,
}

impl ApprovalMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(Self::Auto),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use sherwood_core::{Asset, OrderId, Venue};

    fn order() -> Order {
        Order {
            id: OrderId::new("o1"),
            asset: Asset::symbol("ROAR"),
            side: Side::Buy,
            qty: dec!(2),
            limit_price: Some(dec!(100)),
            max_slippage: dec!(0),
            venue: Venue::RobinhoodMcp,
            reason: "agent tool call: place_order".into(),
        }
    }

    #[tokio::test]
    async fn approve_resolves_the_waiting_ticket() {
        let store = ApprovalStore::new(Duration::from_secs(5));
        let ticket = store.enqueue("place_order", &order());
        let id = ticket.id.clone();
        let waiter = tokio::spawn(ticket.wait());

        assert_eq!(store.pending_count(), 1);
        assert!(store.decide(&id, ApprovalState::Approved, None).unwrap());
        assert_eq!(waiter.await.unwrap(), ApprovalState::Approved);
        assert_eq!(store.pending_count(), 0);
    }

    #[tokio::test]
    async fn deny_resolves_with_a_reason() {
        let store = ApprovalStore::new(Duration::from_secs(5));
        let ticket = store.enqueue("place_order", &order());
        let id = ticket.id.clone();
        let waiter = tokio::spawn(ticket.wait());

        store
            .decide(&id, ApprovalState::Denied, Some("not now".into()))
            .unwrap();
        assert_eq!(waiter.await.unwrap(), ApprovalState::Denied);
        let listed = store.list();
        assert_eq!(listed[0].decision_reason.as_deref(), Some("not now"));
    }

    #[tokio::test]
    async fn ticket_times_out_to_expired() {
        let store = ApprovalStore::new(Duration::from_millis(40));
        let ticket = store.enqueue("place_order", &order());
        assert_eq!(ticket.wait().await, ApprovalState::Expired);
    }

    #[tokio::test]
    async fn sweep_marks_stale_pending_expired() {
        let store = ApprovalStore::new(Duration::from_millis(20));
        let ticket = store.enqueue("place_order", &order());
        let id = ticket.id.clone();
        drop(ticket); // nobody waiting
        tokio::time::sleep(Duration::from_millis(40)).await;
        store.sweep_expired();
        assert_eq!(store.list()[0].state, ApprovalState::Expired);
        // A late decision on an expired approval is a no-op.
        assert!(!store.decide(&id, ApprovalState::Approved, None).unwrap());
    }

    #[tokio::test]
    async fn decide_rejects_a_non_terminal_state() {
        let store = ApprovalStore::new(Duration::from_secs(1));
        assert!(store.decide("x", ApprovalState::Pending, None).is_err());
    }

    #[test]
    fn mode_parses() {
        assert_eq!(ApprovalMode::parse("auto"), Some(ApprovalMode::Auto));
        assert_eq!(ApprovalMode::parse("manual"), Some(ApprovalMode::Manual));
        assert_eq!(ApprovalMode::parse("nope"), None);
    }
}
