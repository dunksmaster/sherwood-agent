//! Reconciling a transaction the operator already broadcast, against the
//! trade they expected
//! ([ADR-0007](../../../docs/adr/0007-no-broadcast-capability.md), v0.2.8).
//!
//! `sherwood-dex` builds calldata; the operator simulates it
//! (`sherwood dex-simulate`), signs it (`sherwood-signer`), and broadcasts
//! it with their own tooling, outside this codebase. What comes back is a
//! transaction hash. This crate takes that hash and reads its receipt
//! ([`sherwood_chain::EvmClient::get_transaction_receipt`]) — never sends
//! anything, has no signer, sees no private key. On a successful receipt it
//! produces a [`sherwood_core::Fill`], the same type the paper executor
//! produces, so the local portfolio and audit trail can reflect what
//! actually happened on-chain.
//!
//! A tx hash and an expected trade are trusted no further than the receipt
//! itself: a wrong hash reconciles to whatever that hash's receipt says (or
//! [`Outcome::NotFound`]), never to what the operator meant to send.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use chrono::Utc;
use rust_decimal::Decimal;
use sherwood_chain::{ChainError, EvmClient, TxReceipt};
use sherwood_core::{Asset, Fill, OrderId, Side, Venue};
use std::time::Duration;

/// The trade the operator expected this transaction to be. Reconciliation
/// does not verify this against the transaction's calldata (the receipt
/// carries no calldata) — it is the label attached to the receipt's outcome
/// for the local record. Get this wrong and the *local* record is wrong;
/// the chain itself is unaffected either way.
#[derive(Debug, Clone)]
pub struct ExpectedTrade {
    pub symbol: String,
    pub side: Side,
    pub qty: Decimal,
    pub price: Decimal,
    /// Fee to record, in the portfolio's cash asset. `sherwood-reconcile`
    /// does not compute this from gas — quote it in cash terms yourself
    /// (e.g. `gas_used * gas_price` converted to your denom) if you want it
    /// tracked; `0` is fine otherwise.
    pub fee: Decimal,
}

/// What reconciling a transaction hash found.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Outcome {
    /// No receipt yet — still pending, or the hash is unknown to this node.
    NotFound,
    /// Mined, but reverted. Gas was still spent; nothing to record as a
    /// fill.
    Reverted(TxReceipt),
    /// Mined and succeeded. `fill` is ready to hand to a portfolio / store,
    /// exactly like a paper fill.
    Confirmed { receipt: TxReceipt, fill: Fill },
}

/// One reconciliation attempt: read the receipt once, and label it against
/// `expected` if it succeeded. Does not poll — see [`wait_and_reconcile`]
/// for that.
pub async fn reconcile<C: EvmClient + ?Sized>(
    client: &C,
    tx_hash: &str,
    expected: &ExpectedTrade,
) -> Result<Outcome, ChainError> {
    match client.get_transaction_receipt(tx_hash).await? {
        None => Ok(Outcome::NotFound),
        Some(receipt) if !receipt.status_ok => Ok(Outcome::Reverted(receipt)),
        Some(receipt) => {
            let fill = Fill {
                order_id: OrderId::new(format!("reconcile-{}", receipt.tx_hash)),
                asset: Asset::symbol(&expected.symbol),
                side: expected.side,
                qty: expected.qty,
                price: expected.price,
                fee: expected.fee,
                venue: Venue::DexRouter,
                at: Utc::now(),
            };
            Ok(Outcome::Confirmed { receipt, fill })
        }
    }
}

/// Poll [`reconcile`] until it stops returning [`Outcome::NotFound`], or
/// `timeout` elapses. A timeout is not a failure of the transaction — it
/// may still confirm later; the caller decides whether to keep waiting.
pub async fn wait_and_reconcile<C: EvmClient + ?Sized>(
    client: &C,
    tx_hash: &str,
    expected: &ExpectedTrade,
    poll_interval: Duration,
    timeout: Duration,
) -> Result<Outcome, ChainError> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match reconcile(client, tx_hash, expected).await? {
            Outcome::NotFound if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(poll_interval).await;
            }
            outcome => return Ok(outcome),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use rust_decimal_macros::dec;
    use serde_json::{json, Value};
    use std::sync::Mutex;

    struct ScriptedClient(Mutex<Vec<sherwood_chain::Result<Value>>>);
    impl ScriptedClient {
        fn new(mut replies: Vec<sherwood_chain::Result<Value>>) -> Self {
            replies.reverse();
            Self(Mutex::new(replies))
        }
    }
    #[async_trait]
    impl EvmClient for ScriptedClient {
        async fn request(&self, _method: &str, _params: Value) -> sherwood_chain::Result<Value> {
            self.0
                .lock()
                .unwrap()
                .pop()
                .expect("no more scripted replies")
        }
    }

    fn expected() -> ExpectedTrade {
        ExpectedTrade {
            symbol: "NVDA".into(),
            side: Side::Buy,
            qty: dec!(0.5),
            price: dec!(200),
            fee: dec!(0.10),
        }
    }

    #[tokio::test]
    async fn not_yet_mined_is_not_found() {
        let client = ScriptedClient::new(vec![Ok(Value::Null)]);
        let outcome = reconcile(&client, "0xabc", &expected()).await.unwrap();
        assert!(matches!(outcome, Outcome::NotFound));
    }

    #[tokio::test]
    async fn a_reverted_transaction_produces_no_fill() {
        let client = ScriptedClient::new(vec![Ok(json!({
            "transactionHash": "0xabc",
            "status": "0x0",
            "blockNumber": "0x2a",
            "gasUsed": "0x5208",
            "to": null,
        }))]);
        let outcome = reconcile(&client, "0xabc", &expected()).await.unwrap();
        assert!(matches!(outcome, Outcome::Reverted(_)));
    }

    #[tokio::test]
    async fn a_successful_transaction_produces_the_expected_fill() {
        let client = ScriptedClient::new(vec![Ok(json!({
            "transactionHash": "0xabc",
            "status": "0x1",
            "blockNumber": "0x2a",
            "gasUsed": "0x5208",
            "to": "0x8876789976decbfcbbbe364623c63652db8c0904",
        }))]);
        let outcome = reconcile(&client, "0xabc", &expected()).await.unwrap();
        let Outcome::Confirmed { receipt, fill } = outcome else {
            panic!("expected Confirmed, got {outcome:?}");
        };
        assert!(receipt.status_ok);
        assert_eq!(fill.asset.symbol, "NVDA");
        assert_eq!(fill.side, Side::Buy);
        assert_eq!(fill.qty, dec!(0.5));
        assert_eq!(fill.price, dec!(200));
        assert_eq!(fill.fee, dec!(0.10));
        assert_eq!(fill.venue, Venue::DexRouter);
        assert_eq!(fill.order_id.0, "reconcile-0xabc");
    }

    #[tokio::test]
    async fn wait_and_reconcile_polls_past_not_found() {
        let client = ScriptedClient::new(vec![
            Ok(Value::Null),
            Ok(Value::Null),
            Ok(json!({
                "transactionHash": "0xabc",
                "status": "0x1",
                "blockNumber": "0x2a",
                "gasUsed": "0x5208",
                "to": null,
            })),
        ]);
        let outcome = wait_and_reconcile(
            &client,
            "0xabc",
            &expected(),
            Duration::from_millis(1),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert!(matches!(outcome, Outcome::Confirmed { .. }));
    }

    #[tokio::test]
    async fn wait_and_reconcile_gives_up_as_not_found_after_timeout() {
        let client = ScriptedClient::new(
            std::iter::repeat_with(|| Ok(Value::Null))
                .take(1000)
                .collect(),
        );
        let outcome = wait_and_reconcile(
            &client,
            "0xabc",
            &expected(),
            Duration::from_millis(1),
            Duration::from_millis(20),
        )
        .await
        .unwrap();
        assert!(matches!(outcome, Outcome::NotFound));
    }
}
