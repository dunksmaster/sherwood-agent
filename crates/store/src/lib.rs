//! Durable state for sherwood-agent: portfolio snapshots, the fill history, and
//! a tamper-evident audit log, on SQLite via `sqlx`.
//!
//! Everything goes through the [`Store`] trait. [`SqliteStore`] is the only
//! implementation; tests use an in-memory database rather than a second mock
//! implementation.
//!
//! The audit log is a hash chain. Each row's `hash` folds in the previous row's
//! `hash`, so deleting or editing any row breaks verification from that point
//! on. See [`SqliteStore::verify_audit_chain`].
//!
//! [`StoreSubscriber`] connects the store to the [event bus](sherwood_events):
//! attach it and every fill and gate rejection is persisted without the
//! producer knowing the store exists.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sherwood_core::{Asset, Fill, OrderId, Portfolio, Side, Venue};
use sherwood_events::{Envelope, Event, Subscriber, SubscriberError};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

/// `prev_hash` of the first audit row — 64 hex zeros.
pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("serialisation error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("corrupt row: {0}")]
    Corrupt(String),
}

/// One entry in the audit hash chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    /// 1-based, contiguous, assigned by the store.
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub kind: String,
    /// Arbitrary payload, stored as canonical (key-sorted) JSON.
    pub data: serde_json::Value,
    pub prev_hash: String,
    pub hash: String,
}

/// Result of walking the whole audit chain and recomputing every hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditVerification {
    Ok {
        entries: i64,
    },
    Broken {
        at_seq: i64,
        expected: String,
        found: String,
    },
}

fn audit_hash(prev_hash: &str, seq: i64, at: &str, kind: &str, data_json: &str) -> String {
    let mut h = Sha256::new();
    for part in [prev_hash, &seq.to_string(), at, kind, data_json] {
        h.update(part.as_bytes());
        h.update(b"\n");
    }
    hex::encode(h.finalize())
}

fn parse_ts(s: &str) -> Result<DateTime<Utc>, StoreError> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| StoreError::Corrupt(format!("timestamp {s:?}: {e}")))
}

fn side_str(s: Side) -> &'static str {
    match s {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

fn parse_side(s: &str) -> Result<Side, StoreError> {
    match s {
        "buy" => Ok(Side::Buy),
        "sell" => Ok(Side::Sell),
        other => Err(StoreError::Corrupt(format!("side {other:?}"))),
    }
}

fn venue_str(v: Venue) -> Result<String, StoreError> {
    match serde_json::to_value(v)? {
        serde_json::Value::String(s) => Ok(s),
        other => Err(StoreError::Corrupt(format!("venue serialised as {other}"))),
    }
}

fn parse_venue(s: &str) -> Result<Venue, StoreError> {
    Ok(serde_json::from_value(serde_json::Value::String(
        s.to_string(),
    ))?)
}

#[async_trait]
pub trait Store: Send + Sync {
    /// Write a snapshot. The most recent snapshot is the authoritative balance
    /// on restart.
    async fn save_portfolio(&self, portfolio: &Portfolio) -> Result<(), StoreError>;

    /// The most recent snapshot, or `None` if none has been written.
    async fn load_portfolio(&self) -> Result<Option<Portfolio>, StoreError>;

    /// Append one fill to the history.
    async fn append_fill(&self, fill: &Fill) -> Result<(), StoreError>;

    /// Every fill, oldest first.
    async fn fills(&self) -> Result<Vec<Fill>, StoreError>;

    /// Append one event to the tamper-evident chain and return the stored row.
    async fn append_audit(
        &self,
        kind: &str,
        data: serde_json::Value,
    ) -> Result<AuditEvent, StoreError>;

    /// The last `n` audit events, oldest first.
    async fn audit_tail(&self, n: i64) -> Result<Vec<AuditEvent>, StoreError>;

    /// Recompute the chain from genesis and confirm every stored hash matches.
    async fn verify_audit_chain(&self) -> Result<AuditVerification, StoreError>;
}

pub struct SqliteStore {
    pool: SqlitePool,
}

impl SqliteStore {
    /// Open (creating if absent) a database file and run migrations.
    pub async fn open(path: &Path) -> Result<Self, StoreError> {
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true);
        Self::from_opts(opts).await
    }

    /// A private in-memory database, for tests.
    pub async fn open_in_memory() -> Result<Self, StoreError> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")?;
        Self::from_opts(opts).await
    }

    async fn from_opts(opts: SqliteConnectOptions) -> Result<Self, StoreError> {
        // One connection: SQLite is single-writer, and `:memory:` gives each
        // connection its own database.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }
}

#[async_trait]
impl Store for SqliteStore {
    async fn save_portfolio(&self, portfolio: &Portfolio) -> Result<(), StoreError> {
        let taken_at = Utc::now().to_rfc3339();
        let state_json = serde_json::to_string(portfolio)?;
        sqlx::query!(
            "INSERT INTO portfolio_snapshots (taken_at, state_json) VALUES (?, ?)",
            taken_at,
            state_json
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn load_portfolio(&self) -> Result<Option<Portfolio>, StoreError> {
        let row =
            sqlx::query!("SELECT state_json FROM portfolio_snapshots ORDER BY id DESC LIMIT 1")
                .fetch_optional(&self.pool)
                .await?;
        match row {
            Some(r) => Ok(Some(serde_json::from_str(&r.state_json)?)),
            None => Ok(None),
        }
    }

    async fn append_fill(&self, fill: &Fill) -> Result<(), StoreError> {
        let side = side_str(fill.side);
        let qty = fill.qty.to_string();
        let price = fill.price.to_string();
        let fee = fill.fee.to_string();
        let venue = venue_str(fill.venue)?;
        let filled_at = fill.at.to_rfc3339();
        let recorded_at = Utc::now().to_rfc3339();
        let order_id = fill.order_id.0.clone();
        let symbol = fill.asset.symbol.clone();
        let address = fill.asset.address.clone();
        sqlx::query!(
            "INSERT INTO fills
               (order_id, symbol, address, side, qty, price, fee, venue, filled_at, recorded_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            order_id,
            symbol,
            address,
            side,
            qty,
            price,
            fee,
            venue,
            filled_at,
            recorded_at,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn fills(&self) -> Result<Vec<Fill>, StoreError> {
        let rows = sqlx::query!(
            "SELECT order_id, symbol, address, side, qty, price, fee, venue, filled_at
             FROM fills ORDER BY id ASC"
        )
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let dec = |s: &str, what: &str| {
                rust_decimal::Decimal::from_str(s)
                    .map_err(|e| StoreError::Corrupt(format!("{what} {s:?}: {e}")))
            };
            out.push(Fill {
                order_id: OrderId::new(r.order_id),
                asset: match r.address {
                    Some(addr) => Asset::onchain(r.symbol, addr),
                    None => Asset::symbol(r.symbol),
                },
                side: parse_side(&r.side)?,
                qty: dec(&r.qty, "qty")?,
                price: dec(&r.price, "price")?,
                fee: dec(&r.fee, "fee")?,
                venue: parse_venue(&r.venue)?,
                at: parse_ts(&r.filled_at)?,
            });
        }
        Ok(out)
    }

    async fn append_audit(
        &self,
        kind: &str,
        data: serde_json::Value,
    ) -> Result<AuditEvent, StoreError> {
        // `serde_json::Value` orders object keys (BTreeMap), so this string is
        // canonical for a given value.
        let data_json = serde_json::to_string(&data)?;
        let at = Utc::now();
        let at_s = at.to_rfc3339();

        let mut tx = self.pool.begin().await?;
        let last = sqlx::query!(
            r#"SELECT seq AS "seq!: i64", hash FROM audit_log ORDER BY seq DESC LIMIT 1"#
        )
        .fetch_optional(&mut *tx)
        .await?;
        let (seq, prev_hash) = match last {
            Some(r) => (r.seq + 1, r.hash),
            None => (1, GENESIS_HASH.to_string()),
        };
        let hash = audit_hash(&prev_hash, seq, &at_s, kind, &data_json);
        sqlx::query!(
            "INSERT INTO audit_log (seq, at, kind, data_json, prev_hash, hash)
             VALUES (?, ?, ?, ?, ?, ?)",
            seq,
            at_s,
            kind,
            data_json,
            prev_hash,
            hash,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        Ok(AuditEvent {
            seq,
            at,
            kind: kind.to_string(),
            data,
            prev_hash,
            hash,
        })
    }

    async fn audit_tail(&self, n: i64) -> Result<Vec<AuditEvent>, StoreError> {
        let rows = sqlx::query!(
            r#"SELECT seq AS "seq!: i64", at, kind, data_json, prev_hash, hash
               FROM audit_log ORDER BY seq DESC LIMIT ?"#,
            n
        )
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows.into_iter().rev() {
            out.push(AuditEvent {
                seq: r.seq,
                at: parse_ts(&r.at)?,
                kind: r.kind,
                data: serde_json::from_str(&r.data_json)?,
                prev_hash: r.prev_hash,
                hash: r.hash,
            });
        }
        Ok(out)
    }

    async fn verify_audit_chain(&self) -> Result<AuditVerification, StoreError> {
        let rows = sqlx::query!(
            r#"SELECT seq AS "seq!: i64", at, kind, data_json, prev_hash, hash
               FROM audit_log ORDER BY seq ASC"#
        )
        .fetch_all(&self.pool)
        .await?;

        let mut expected_prev = GENESIS_HASH.to_string();
        let mut count = 0i64;
        for (i, r) in rows.iter().enumerate() {
            let want_seq = i as i64 + 1;
            if r.seq != want_seq {
                return Ok(AuditVerification::Broken {
                    at_seq: r.seq,
                    expected: format!("seq {want_seq}"),
                    found: format!("seq {}", r.seq),
                });
            }
            if r.prev_hash != expected_prev {
                return Ok(AuditVerification::Broken {
                    at_seq: r.seq,
                    expected: expected_prev,
                    found: r.prev_hash.clone(),
                });
            }
            let recomputed = audit_hash(&r.prev_hash, r.seq, &r.at, &r.kind, &r.data_json);
            if recomputed != r.hash {
                return Ok(AuditVerification::Broken {
                    at_seq: r.seq,
                    expected: recomputed,
                    found: r.hash.clone(),
                });
            }
            expected_prev = r.hash.clone();
            count += 1;
        }
        Ok(AuditVerification::Ok { entries: count })
    }
}

/// Persists events from the [bus](sherwood_events) into the store: fills to the
/// `fills` table, and `fill` / `decision` / `gate_reject` / `run_end` rows to
/// the audit chain. The portfolio snapshot is written by the run loop directly,
/// since it owns that state — the bus carries what other components need to
/// observe, not everything.
pub struct StoreSubscriber {
    store: Arc<SqliteStore>,
}

impl StoreSubscriber {
    pub fn new(store: Arc<SqliteStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Subscriber for StoreSubscriber {
    async fn handle(&mut self, env: &Envelope) -> Result<(), SubscriberError> {
        match &env.event {
            Event::OrderFilled(fill) => {
                self.store.append_fill(fill).await?;
                self.store
                    .append_audit(
                        "fill",
                        json!({
                            "order_id": fill.order_id.0,
                            "symbol": fill.asset.symbol,
                            "side": side_str(fill.side),
                            "qty": fill.qty.to_string(),
                            "price": fill.price.to_string(),
                            "fee": fill.fee.to_string(),
                        }),
                    )
                    .await?;
            }
            Event::RiskRejected {
                order_id,
                symbol,
                reason,
            } => {
                self.store
                    .append_audit(
                        "gate_reject",
                        json!({ "order_id": order_id.0, "symbol": symbol, "reason": reason }),
                    )
                    .await?;
            }
            Event::Decided {
                tick,
                price,
                decision,
            } => {
                self.store
                    .append_audit(
                        "decision",
                        json!({ "tick": tick, "price": price.to_string(), "decision": decision }),
                    )
                    .await?;
            }
            Event::RunEnded {
                label,
                interrupted,
                cash,
                realized_pnl,
            } => {
                self.store
                    .append_audit(
                        "run_end",
                        json!({
                            "label": label,
                            "state": if *interrupted { "interrupted" } else { "done" },
                            "cash": cash.to_string(),
                            "realized_pnl": realized_pnl.to_string(),
                        }),
                    )
                    .await?;
            }
        }
        Ok(())
    }

    fn name(&self) -> &'static str {
        "store"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use serde_json::json;
    use sherwood_core::Venue;

    fn fill(
        sym: &str,
        side: Side,
        qty: rust_decimal::Decimal,
        price: rust_decimal::Decimal,
    ) -> Fill {
        Fill {
            order_id: OrderId::new("o-1"),
            asset: Asset::symbol(sym),
            side,
            qty,
            price,
            fee: dec!(0.25),
            venue: Venue::Paper,
            at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn empty_store_has_no_portfolio() {
        let s = SqliteStore::open_in_memory().await.unwrap();
        assert!(s.load_portfolio().await.unwrap().is_none());
        assert_eq!(s.fills().await.unwrap().len(), 0);
        assert_eq!(
            s.verify_audit_chain().await.unwrap(),
            AuditVerification::Ok { entries: 0 }
        );
    }

    #[tokio::test]
    async fn portfolio_and_fills_survive_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");

        let mut portfolio = Portfolio::new(dec!(1000));
        {
            let s = SqliteStore::open(&path).await.unwrap();
            portfolio.apply(&fill("ROAR", Side::Buy, dec!(4), dec!(11)));
            portfolio.apply(&fill("ROAR", Side::Sell, dec!(1), dec!(20)));
            s.append_fill(&fill("ROAR", Side::Buy, dec!(4), dec!(11)))
                .await
                .unwrap();
            s.append_fill(&fill("ROAR", Side::Sell, dec!(1), dec!(20)))
                .await
                .unwrap();
            s.save_portfolio(&portfolio).await.unwrap();
        } // store (and its connection) dropped here

        let s = SqliteStore::open(&path).await.unwrap();
        assert_eq!(s.load_portfolio().await.unwrap().unwrap(), portfolio);
        let fills = s.fills().await.unwrap();
        assert_eq!(fills.len(), 2);
        assert_eq!(fills[0].side, Side::Buy);
        assert_eq!(fills[1].price, dec!(20));
        assert_eq!(fills[0].fee, dec!(0.25));
    }

    #[tokio::test]
    async fn latest_snapshot_wins() {
        let s = SqliteStore::open_in_memory().await.unwrap();
        s.save_portfolio(&Portfolio::new(dec!(100))).await.unwrap();
        s.save_portfolio(&Portfolio::new(dec!(999))).await.unwrap();
        let loaded = s.load_portfolio().await.unwrap().unwrap();
        assert_eq!(loaded, Portfolio::new(dec!(999)));
    }

    #[tokio::test]
    async fn audit_chain_links_and_verifies() {
        let s = SqliteStore::open_in_memory().await.unwrap();
        let a = s
            .append_audit("decision", json!({"symbol": "ROAR", "action": "buy"}))
            .await
            .unwrap();
        let b = s
            .append_audit("fill", json!({"qty": "4", "price": "11"}))
            .await
            .unwrap();
        let c = s
            .append_audit("gate_reject", json!({"reason": "notional cap"}))
            .await
            .unwrap();

        assert_eq!(a.seq, 1);
        assert_eq!(a.prev_hash, GENESIS_HASH);
        assert_eq!(b.prev_hash, a.hash);
        assert_eq!(c.prev_hash, b.hash);
        assert_eq!(
            s.verify_audit_chain().await.unwrap(),
            AuditVerification::Ok { entries: 3 }
        );
    }

    #[tokio::test]
    async fn audit_tampering_is_detected() {
        let s = SqliteStore::open_in_memory().await.unwrap();
        s.append_audit("a", json!({"n": 1})).await.unwrap();
        s.append_audit("b", json!({"n": 2})).await.unwrap();
        s.append_audit("c", json!({"n": 3})).await.unwrap();

        // Edit row 2's payload directly, behind the Store API's back.
        sqlx::query!(
            "UPDATE audit_log SET data_json = ? WHERE seq = 2",
            r#"{"n":99}"#
        )
        .execute(&s.pool)
        .await
        .unwrap();

        match s.verify_audit_chain().await.unwrap() {
            AuditVerification::Broken { at_seq, .. } => assert_eq!(at_seq, 2),
            AuditVerification::Ok { .. } => panic!("tamper not detected"),
        }
    }

    #[tokio::test]
    async fn audit_tail_returns_last_n_oldest_first() {
        let s = SqliteStore::open_in_memory().await.unwrap();
        for i in 1..=5 {
            s.append_audit("t", json!({ "i": i })).await.unwrap();
        }
        let tail = s.audit_tail(3).await.unwrap();
        let seqs: Vec<i64> = tail.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![3, 4, 5]);
        assert_eq!(tail[0].data, json!({"i": 3}));
    }

    #[tokio::test]
    async fn onchain_asset_address_round_trips() {
        let s = SqliteStore::open_in_memory().await.unwrap();
        let mut f = fill("PONS", Side::Buy, dec!(10), dec!(1));
        f.asset = Asset::onchain("PONS", "0xabc123");
        s.append_fill(&f).await.unwrap();
        let got = s.fills().await.unwrap();
        assert_eq!(got[0].asset.address.as_deref(), Some("0xabc123"));
    }

    #[tokio::test]
    async fn store_subscriber_persists_bus_events() {
        use sherwood_events::{run_subscriber, Bus};

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        let store = Arc::new(SqliteStore::open(&path).await.unwrap());
        let bus = Bus::new(64);
        let handle = tokio::spawn(run_subscriber(
            bus.subscribe(),
            StoreSubscriber::new(store.clone()),
        ));

        bus.publish(Event::Decided {
            tick: 0,
            price: dec!(100),
            decision: "buy".into(),
        });
        bus.publish(Event::OrderFilled(fill(
            "ROAR",
            Side::Buy,
            dec!(3),
            dec!(10),
        )));
        bus.publish(Event::RiskRejected {
            order_id: OrderId::new("o-2"),
            symbol: "ROAR".into(),
            reason: "notional cap".into(),
        });
        bus.publish(Event::RunEnded {
            label: "t".into(),
            interrupted: false,
            cash: dec!(970),
            realized_pnl: dec!(0),
        });

        drop(bus); // close the channel; the subscriber drains and returns
        handle.await.unwrap();

        assert_eq!(store.fills().await.unwrap().len(), 1);
        assert!(matches!(
            store.verify_audit_chain().await.unwrap(),
            AuditVerification::Ok { entries: 4 }
        ));
        let kinds: Vec<String> = store
            .audit_tail(4)
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.kind)
            .collect();
        assert_eq!(kinds, ["decision", "fill", "gate_reject", "run_end"]);
    }
}
