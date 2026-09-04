//! Price feeds.
//!
//! This module holds only the *shape* — the [`Tick`] type and the [`PriceFeed`]
//! trait. Concrete feeds do I/O and live outside `core`: a CSV replay and an
//! in-memory feed are in `sherwood-cli` today; a websocket / Geyser feed gets
//! its own crate when v0.2 needs it.

use crate::types::Asset;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

/// One observation: a price for a symbol at an instant.
#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    pub at: DateTime<Utc>,
    pub symbol: String,
    pub price: Decimal,
}

impl Tick {
    pub fn asset(&self) -> Asset {
        Asset::symbol(self.symbol.clone())
    }
}

/// A source of [`Tick`]s.
///
/// Ticks are delivered in non-decreasing `at` order. `next_tick` returns `None`
/// once the feed is exhausted (a bounded replay) — a live feed simply never
/// returns `None`.
pub trait PriceFeed: Send {
    fn next_tick(&mut self) -> Option<Tick>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    struct VecFeed(std::vec::IntoIter<Tick>);
    impl PriceFeed for VecFeed {
        fn next_tick(&mut self) -> Option<Tick> {
            self.0.next()
        }
    }

    #[test]
    fn feed_yields_ticks_then_stops() {
        let t = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        let mut f = VecFeed(
            vec![
                Tick {
                    at: t,
                    symbol: "A".into(),
                    price: dec!(1),
                },
                Tick {
                    at: t,
                    symbol: "B".into(),
                    price: dec!(2),
                },
            ]
            .into_iter(),
        );
        assert_eq!(f.next_tick().unwrap().symbol, "A");
        assert_eq!(f.next_tick().unwrap().symbol, "B");
        assert!(f.next_tick().is_none());
    }
}
