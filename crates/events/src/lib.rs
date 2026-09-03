//! The internal event bus.
//!
//! Components publish [`Event`]s and never call each other directly. The bus is
//! a bounded `tokio::sync::broadcast` channel; subscribers each get their own
//! receiver. A slow or failing subscriber is logged and skipped — it can never
//! take the bus, or another subscriber, down with it.
//!
//! Every message is wrapped in an [`Envelope`] carrying a schema `version`, so
//! that adding or changing a variant later is a versioned, detectable change
//! rather than a silent one. See `docs/RUNTIME.md`.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sherwood_core::{Fill, OrderId};
use tokio::sync::broadcast;

/// Bumped whenever the shape of [`Event`] or [`Envelope`] changes in a way a
/// consumer could observe. Consumers may reject an envelope whose version they
/// do not understand.
pub const EVENT_SCHEMA_VERSION: u16 = 1;

/// Something that happened, worth telling other components about.
///
/// Variants exist only when they have a real emitter *and* a real consumer.
/// Decimals travel as `Decimal`, consistent with the rest of the system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    /// The decision layer produced a non-`Hold` verdict for a tick.
    Decided {
        tick: u32,
        price: Decimal,
        decision: String,
    },
    /// An order cleared the risk gate and filled.
    OrderFilled(Fill),
    /// The risk gate refused an order.
    RiskRejected {
        order_id: OrderId,
        symbol: String,
        reason: String,
    },
    /// A run finished — cleanly or by interrupt.
    RunEnded {
        label: String,
        interrupted: bool,
        cash: Decimal,
        realized_pnl: Decimal,
    },
}

impl Event {
    /// The id that ties this event to a single trade's trail, where one exists.
    fn correlation_id(&self) -> String {
        match self {
            Event::OrderFilled(f) => f.order_id.0.clone(),
            Event::RiskRejected { order_id, .. } => order_id.0.clone(),
            Event::Decided { tick, .. } => format!("tick-{tick}"),
            Event::RunEnded { label, .. } => format!("run-{label}"),
        }
    }

    /// Short, stable label for logs and metrics.
    pub fn kind(&self) -> &'static str {
        match self {
            Event::Decided { .. } => "decided",
            Event::OrderFilled(_) => "order_filled",
            Event::RiskRejected { .. } => "risk_rejected",
            Event::RunEnded { .. } => "run_ended",
        }
    }
}

/// An [`Event`] plus the metadata every event carries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub version: u16,
    pub at: DateTime<Utc>,
    pub correlation_id: String,
    pub event: Event,
}

/// A cloneable handle to the bus. Cloning gives another publisher; the bus
/// stays open until every `Bus` handle is dropped.
#[derive(Clone)]
pub struct Bus {
    tx: broadcast::Sender<Envelope>,
}

impl Bus {
    /// `capacity` is the number of unconsumed events a subscriber may fall
    /// behind before it starts losing the oldest ones (and gets a lag warning).
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Publish an event. Does nothing if there are no subscribers — that is not
    /// an error.
    pub fn publish(&self, event: Event) {
        let env = Envelope {
            version: EVENT_SCHEMA_VERSION,
            at: Utc::now(),
            correlation_id: event.correlation_id(),
            event,
        };
        let _ = self.tx.send(env);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Envelope> {
        self.tx.subscribe()
    }

    /// Number of live subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self::new(1000)
    }
}

/// Error type a subscriber's `handle` may return. Boxed so a subscriber can
/// surface any concrete error without the bus crate depending on it.
pub type SubscriberError = Box<dyn std::error::Error + Send + Sync>;

/// A consumer of bus events.
#[async_trait]
pub trait Subscriber: Send {
    async fn handle(&mut self, env: &Envelope) -> Result<(), SubscriberError>;

    /// Stable name for logs.
    fn name(&self) -> &'static str;
}

/// Drive `sub` until the bus closes (every [`Bus`] handle dropped).
///
/// A handler error or a lagged receiver is logged and the loop continues — a
/// subscriber must never take the bus down. Returns once the bus is closed and
/// the backlog is drained, so a caller can `await` this to flush on shutdown.
pub async fn run_subscriber(mut rx: broadcast::Receiver<Envelope>, mut sub: impl Subscriber) {
    loop {
        match rx.recv().await {
            Ok(env) => {
                if let Err(e) = sub.handle(&env).await {
                    tracing::warn!(
                        subscriber = sub.name(),
                        error = %e,
                        kind = env.event.kind(),
                        "subscriber handler failed; continuing"
                    );
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(
                    subscriber = sub.name(),
                    dropped = n,
                    "subscriber fell behind; events were dropped"
                );
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// A subscriber that emits one structured log line per event. Always safe to
/// attach; the observability step scrapes this stream.
#[derive(Default)]
pub struct TracingSubscriber;

#[async_trait]
impl Subscriber for TracingSubscriber {
    async fn handle(&mut self, env: &Envelope) -> Result<(), SubscriberError> {
        tracing::info!(
            target: "sherwood::events",
            kind = env.event.kind(),
            correlation_id = %env.correlation_id,
            version = env.version,
            "event"
        );
        Ok(())
    }

    fn name(&self) -> &'static str {
        "tracing"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    fn run_ended() -> Event {
        Event::RunEnded {
            label: "t".into(),
            interrupted: false,
            cash: dec!(1000),
            realized_pnl: dec!(0),
        }
    }

    struct Counter(Arc<AtomicUsize>);

    #[async_trait]
    impl Subscriber for Counter {
        async fn handle(&mut self, _env: &Envelope) -> Result<(), SubscriberError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn name(&self) -> &'static str {
            "counter"
        }
    }

    #[tokio::test]
    async fn delivers_to_every_subscriber() {
        let bus = Bus::new(16);
        let a = Arc::new(AtomicUsize::new(0));
        let b = Arc::new(AtomicUsize::new(0));
        let ha = tokio::spawn(run_subscriber(bus.subscribe(), Counter(a.clone())));
        let hb = tokio::spawn(run_subscriber(bus.subscribe(), Counter(b.clone())));

        for _ in 0..5 {
            bus.publish(run_ended());
        }
        drop(bus); // close the channel so the subscribers finish
        ha.await.unwrap();
        hb.await.unwrap();

        assert_eq!(a.load(Ordering::SeqCst), 5);
        assert_eq!(b.load(Ordering::SeqCst), 5);
    }

    #[tokio::test]
    async fn publish_with_no_subscribers_is_not_an_error() {
        let bus = Bus::new(4);
        bus.publish(run_ended()); // must not panic
        assert_eq!(bus.subscriber_count(), 0);
    }

    #[tokio::test]
    async fn a_failing_handler_does_not_stop_the_subscriber() {
        struct Flaky {
            seen: usize,
        }
        #[async_trait]
        impl Subscriber for Flaky {
            async fn handle(&mut self, _env: &Envelope) -> Result<(), SubscriberError> {
                self.seen += 1;
                if self.seen == 1 {
                    return Err("boom".into());
                }
                Ok(())
            }
            fn name(&self) -> &'static str {
                "flaky"
            }
        }

        let bus = Bus::new(8);
        let done = Arc::new(AtomicUsize::new(0));
        let d2 = done.clone();
        // Wrap Flaky so the test can observe completion count.
        struct Wrap(Flaky, Arc<AtomicUsize>);
        #[async_trait]
        impl Subscriber for Wrap {
            async fn handle(&mut self, env: &Envelope) -> Result<(), SubscriberError> {
                let r = self.0.handle(env).await;
                self.1.fetch_add(1, Ordering::SeqCst);
                r
            }
            fn name(&self) -> &'static str {
                "wrap"
            }
        }
        let h = tokio::spawn(run_subscriber(bus.subscribe(), Wrap(Flaky { seen: 0 }, d2)));
        bus.publish(run_ended());
        bus.publish(run_ended());
        drop(bus);
        h.await.unwrap();
        assert_eq!(
            done.load(Ordering::SeqCst),
            2,
            "both events were handled despite the first error"
        );
    }

    #[tokio::test]
    async fn envelope_carries_schema_version_and_correlation_id() {
        let bus = Bus::new(4);
        let mut rx = bus.subscribe();
        bus.publish(Event::Decided {
            tick: 3,
            price: dec!(100),
            decision: "buy".into(),
        });
        let env = rx.recv().await.unwrap();
        assert_eq!(env.version, EVENT_SCHEMA_VERSION);
        assert_eq!(env.correlation_id, "tick-3");
        assert_eq!(env.event.kind(), "decided");
    }
}
