//! Concrete price feeds for the CLI: an in-memory replay and a CSV replay.
//! The [`PriceFeed`] trait itself lives in `sherwood-core`.

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sherwood_core::{PriceFeed, Tick};
use std::path::Path;
use std::str::FromStr;

/// Replays a fixed list of ticks, in order.
#[derive(Debug)]
pub struct SliceFeed {
    ticks: std::vec::IntoIter<Tick>,
}

impl SliceFeed {
    pub fn new(ticks: Vec<Tick>) -> Self {
        Self {
            ticks: ticks.into_iter(),
        }
    }
}

impl PriceFeed for SliceFeed {
    fn next_tick(&mut self) -> Option<Tick> {
        self.ticks.next()
    }
}

/// Replays ticks from a CSV file: one `timestamp,symbol,price` row per line,
/// `timestamp` in RFC 3339. A `timestamp,symbol,price` header row is skipped if
/// present; blank lines and `#` comments are ignored.
#[derive(Debug)]
pub struct CsvFeed {
    ticks: std::vec::IntoIter<Tick>,
}

impl CsvFeed {
    pub fn open(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading feed {}", path.display()))?;
        let mut ticks = Vec::new();

        for (n, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut fields = line.split(',');
            let (Some(ts), Some(sym), Some(px), None) =
                (fields.next(), fields.next(), fields.next(), fields.next())
            else {
                bail!(
                    "{}:{}: expected `timestamp,symbol,price`, got {line:?}",
                    path.display(),
                    n + 1
                );
            };
            let (ts, sym, px) = (ts.trim(), sym.trim(), px.trim());
            if ts.eq_ignore_ascii_case("timestamp") {
                continue; // header
            }
            let at = DateTime::parse_from_rfc3339(ts)
                .map(|d| d.with_timezone(&Utc))
                .with_context(|| format!("{}:{}: bad timestamp {ts:?}", path.display(), n + 1))?;
            let price = Decimal::from_str(px)
                .with_context(|| format!("{}:{}: bad price {px:?}", path.display(), n + 1))?;
            ticks.push(Tick {
                at,
                symbol: sym.to_string(),
                price,
            });
        }

        if ticks.is_empty() {
            bail!("feed {} contained no ticks", path.display());
        }
        Ok(Self {
            ticks: ticks.into_iter(),
        })
    }
}

impl PriceFeed for CsvFeed {
    fn next_tick(&mut self) -> Option<Tick> {
        self.ticks.next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use std::io::Write;

    fn write(name: &str, body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(name);
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        (dir, p)
    }

    #[test]
    fn csv_feed_parses_rows_skips_header_and_comments() {
        let (_d, p) = write(
            "f.csv",
            "timestamp,symbol,price\n\
             # a comment\n\
             2026-01-01T00:00:00Z,ROAR,100\n\
             \n\
             2026-01-01T00:01:00Z,HMNI,4.5\n",
        );
        let mut f = CsvFeed::open(&p).unwrap();
        let a = f.next_tick().unwrap();
        assert_eq!((a.symbol.as_str(), a.price), ("ROAR", dec!(100)));
        let b = f.next_tick().unwrap();
        assert_eq!((b.symbol.as_str(), b.price), ("HMNI", dec!(4.5)));
        assert!(f.next_tick().is_none());
    }

    #[test]
    fn csv_feed_rejects_a_malformed_row() {
        let (_d, p) = write("f.csv", "2026-01-01T00:00:00Z,ROAR\n");
        let err = CsvFeed::open(&p).unwrap_err().to_string();
        assert!(err.contains("timestamp,symbol,price"), "{err}");
    }

    #[test]
    fn csv_feed_rejects_a_bad_price() {
        let (_d, p) = write("f.csv", "2026-01-01T00:00:00Z,ROAR,not-a-number\n");
        assert!(CsvFeed::open(&p)
            .unwrap_err()
            .to_string()
            .contains("bad price"));
    }

    #[test]
    fn empty_feed_is_an_error() {
        let (_d, p) = write("f.csv", "# nothing but a comment\n");
        assert!(CsvFeed::open(&p).is_err());
    }

    #[test]
    fn slice_feed_replays_in_order() {
        let t = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        let mut f = SliceFeed::new(vec![
            Tick {
                at: t,
                symbol: "A".into(),
                price: dec!(1),
            },
            Tick {
                at: t,
                symbol: "A".into(),
                price: dec!(2),
            },
        ]);
        assert_eq!(f.next_tick().unwrap().price, dec!(1));
        assert_eq!(f.next_tick().unwrap().price, dec!(2));
        assert!(f.next_tick().is_none());
    }
}
