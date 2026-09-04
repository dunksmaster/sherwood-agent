//! [`ChainFeed`] — a [`sherwood_core::PriceFeed`] backed by live Robinhood
//! Chain Uniswap v4 prices. Still all reads: it holds an [`EvmClient`], never
//! a wallet.
//!
//! `PriceFeed::next_tick` is synchronous (`sherwood_core::feed`'s own doc
//! comment: "a live feed simply never returns `None`"). This bridges that to
//! `sherwood-chain`'s async reads with `tokio::task::block_in_place` +
//! `Handle::block_on`, which requires the caller be on a multi-thread Tokio
//! runtime (`sherwood-cli` is).
//!
//! Pool discovery (an `Initialize`-log scan) is the expensive part of a price
//! read, so it happens once per symbol, the first time that symbol is polled,
//! and the winning pool is cached; every later poll is one cheap `getSlot0`.
//! A read that keeps failing backs off and, past a cap, falls back to
//! sleeping a full poll interval rather than spinning — bounded, never a
//! silent infinite retry, and it never fabricates a price or returns `None`.

use crate::univ4::{self, Decimals, PoolKey};
use crate::{tokens, EvmClient};
use rust_decimal::Decimal;
use sherwood_core::{PriceFeed, Tick};
use std::time::Duration;

/// One symbol this feed polls, plus its pool once discovered.
struct Sym {
    symbol: String,
    address: String,
    decimals: u8,
    pool: Option<PoolKey>,
}

/// Tunables. `Default` points at Robinhood Chain's real deployment and USDG.
#[derive(Debug, Clone)]
pub struct ChainFeedConfig {
    pub pool_manager: String,
    pub state_view: String,
    pub denom_address: String,
    pub denom_decimals: u8,
    /// How often the feed cycles back to the first symbol. Ignored between
    /// symbols within one cycle — those poll back-to-back.
    pub poll_interval: Duration,
    /// Retries before a read gives up and sleeps a full `poll_interval`
    /// instead of continuing to back off.
    pub max_retries: u32,
}

impl Default for ChainFeedConfig {
    fn default() -> Self {
        let (_, denom_address, denom_decimals) = tokens::resolve("USDG");
        Self {
            pool_manager: tokens::POOL_MANAGER.to_owned(),
            state_view: tokens::STATE_VIEW.to_owned(),
            denom_address,
            denom_decimals,
            poll_interval: Duration::from_secs(15),
            max_retries: 8,
        }
    }
}

/// A live [`PriceFeed`] over Robinhood Chain Stock Tokens.
pub struct ChainFeed<C: EvmClient> {
    client: C,
    cfg: ChainFeedConfig,
    symbols: Vec<Sym>,
    idx: usize,
    cycle_deadline: Option<tokio::time::Instant>,
}

impl<C: EvmClient> ChainFeed<C> {
    /// `symbols` accepts known symbols (`"NVDA"`) or raw addresses.
    #[must_use]
    pub fn new(client: C, symbols: &[String], cfg: ChainFeedConfig) -> Self {
        let symbols = symbols
            .iter()
            .map(|s| {
                let (symbol, address, decimals) = tokens::resolve(s);
                Sym {
                    symbol,
                    address,
                    decimals,
                    pool: None,
                }
            })
            .collect();
        Self {
            client,
            cfg,
            symbols,
            idx: 0,
            cycle_deadline: None,
        }
    }
}

impl<C: EvmClient> PriceFeed for ChainFeed<C> {
    fn next_tick(&mut self) -> Option<Tick> {
        if self.symbols.is_empty() {
            return None;
        }
        if self.idx == 0 {
            self.wait_for_cycle();
        }
        let i = self.idx;
        self.idx = (self.idx + 1) % self.symbols.len();

        let (price, pool) = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(read_one(
                &self.client,
                &self.cfg,
                &self.symbols[i],
            ))
        });
        self.symbols[i].pool = Some(pool);

        Some(Tick {
            at: chrono::Utc::now(),
            symbol: self.symbols[i].symbol.clone(),
            price,
        })
    }
}

impl<C: EvmClient> ChainFeed<C> {
    fn wait_for_cycle(&mut self) {
        if let Some(deadline) = self.cycle_deadline {
            let now = tokio::time::Instant::now();
            if now < deadline {
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(tokio::time::sleep_until(deadline));
                });
            }
        }
        self.cycle_deadline = Some(tokio::time::Instant::now() + self.cfg.poll_interval);
    }
}

/// Read one symbol's price, discovering its pool on first use, retrying a
/// failure with backoff and, past `max_retries`, sleeping a full
/// `poll_interval` before trying again — bounded, and it always eventually
/// returns a price rather than hanging forever or fabricating one.
async fn read_one<C: EvmClient>(
    client: &C,
    cfg: &ChainFeedConfig,
    sym: &Sym,
) -> (Decimal, PoolKey) {
    let decimals = Decimals {
        token: sym.decimals,
        denominator: cfg.denom_decimals,
    };
    let mut attempt: u32 = 0;
    loop {
        let pool_result: crate::Result<PoolKey> = match &sym.pool {
            Some(p) => Ok(p.clone()),
            None => {
                (async {
                    let head = client.block_number().await?;
                    let deployment = univ4::Deployment {
                        pool_manager: &cfg.pool_manager,
                        state_view: &cfg.state_view,
                    };
                    let ranked = univ4::find_best_pool(
                        client,
                        deployment,
                        &sym.address,
                        &cfg.denom_address,
                        0,
                        head,
                    )
                    .await?;
                    Ok(ranked.key)
                })
                .await
            }
        };
        let outcome = match pool_result {
            Ok(pool) => univ4::quote_pool(client, &cfg.state_view, &pool, &sym.address, decimals)
                .await
                .map(|price| (price, pool)),
            Err(e) => Err(e),
        };
        match outcome {
            Ok(result) => return result,
            Err(e) => {
                tracing::warn!(symbol = %sym.symbol, %e, attempt, "chain feed: price read failed");
                attempt += 1;
                if attempt > cfg.max_retries {
                    tracing::error!(
                        symbol = %sym.symbol,
                        "chain feed: repeated failures, sleeping a full poll interval"
                    );
                    tokio::time::sleep(cfg.poll_interval.max(Duration::from_secs(5))).await;
                    attempt = 0;
                } else {
                    let backoff = Duration::from_millis(300 * 2u64.pow(attempt.min(6)));
                    tokio::time::sleep(backoff).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::MockRpc;
    use crate::{abi, keccak::topic_hex};
    use serde_json::json;

    fn w(hex: &str) -> serde_json::Value {
        json!(format!("0x{:0>64}", hex))
    }

    fn slot0_reply(sqrt_price_x96: u128) -> crate::Result<serde_json::Value> {
        let mut ret = Vec::new();
        ret.extend_from_slice(&abi::uint_word(sqrt_price_x96));
        ret.extend_from_slice(&abi::int_word(0));
        ret.extend_from_slice(&abi::uint_word(0));
        ret.extend_from_slice(&abi::uint_word(3000));
        Ok(json!(abi::to_hex(&ret)))
    }

    fn init_log(pool_id: &str, currency0: &str, currency1: &str) -> serde_json::Value {
        let mut data = Vec::new();
        data.extend_from_slice(&abi::uint_word(3000));
        data.extend_from_slice(&abi::int_word(60));
        data.extend_from_slice(
            &abi::address_word("0x0000000000000000000000000000000000000000").unwrap(),
        );
        json!({
            "address": "0xpoolmanager",
            "topics": [
                topic_hex("Initialize(bytes32,address,address,uint24,int24,address,uint160,int24)"),
                pool_id,
                format!("0x{}{}", "0".repeat(24), &currency0[2..]),
                format!("0x{}{}", "0".repeat(24), &currency1[2..]),
            ],
            "data": abi::to_hex(&data),
            "blockNumber": "0x10",
        })
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn discovers_once_then_polls_cheaply_thereafter() {
        const NVDA: &str = "0xd0601ce157db5bdc3162bbac2a2c8af5320d9eec";
        const USDG: &str = "0x5fc5360d0400a0fd4f2af552add042d716f1d168";

        let mut replies: Vec<crate::Result<serde_json::Value>> = vec![
            Ok(json!("0x10")), // block_number for discovery
            Ok(json!([init_log(
                "0xd80b3b4cc602181da7004070f75c54b3107807f0cc71072d8c373662d44df972",
                USDG,
                NVDA
            )])), // discover_pools: as currency0
            Ok(json!([])),     // discover_pools: as currency1
            Ok(w("64")),       // rank_by_liquidity: getLiquidity > 0
        ];
        replies.push(slot0_reply(79_228_162_514_264_337_593_543_950_336)); // tick 1: getSlot0 (ratio 1)
        replies.push(slot0_reply(79_228_162_514_264_337_593_543_950_336)); // tick 2: getSlot0 only — no rediscovery

        let rpc = MockRpc::new(replies);
        let cfg = ChainFeedConfig {
            poll_interval: Duration::from_millis(0),
            ..ChainFeedConfig::default()
        };
        let mut feed = ChainFeed::new(rpc, &["NVDA".to_owned()], cfg);

        let t1 = feed.next_tick().unwrap();
        assert_eq!(t1.symbol, "NVDA");
        let t2 = feed.next_tick().unwrap();
        assert_eq!(t2.symbol, "NVDA");
        // Both ticks succeeded using exactly the scripted replies above — a
        // second `Initialize` scan would have exhausted the mock and errored.
        assert!(feed.symbols[0].pool.is_some());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn empty_symbol_list_yields_no_ticks() {
        let rpc = MockRpc::new(vec![]);
        let mut feed = ChainFeed::new(rpc, &[], ChainFeedConfig::default());
        assert!(feed.next_tick().is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_failed_discovery_retries_and_eventually_succeeds() {
        const NVDA: &str = "0xd0601ce157db5bdc3162bbac2a2c8af5320d9eec";
        const USDG: &str = "0x5fc5360d0400a0fd4f2af552add042d716f1d168";
        let init = init_log(
            "0xd80b3b4cc602181da7004070f75c54b3107807f0cc71072d8c373662d44df972",
            USDG,
            NVDA,
        );
        let replies: Vec<crate::Result<serde_json::Value>> = vec![
            // first attempt: discovery's own block_number call fails
            Err(crate::ChainError::Rpc {
                code: -32000,
                message: "boom".into(),
            }),
            // second attempt: succeeds
            Ok(json!("0x10")),
            Ok(json!([init])),
            Ok(json!([])),
            Ok(w("64")),
            slot0_reply(79_228_162_514_264_337_593_543_950_336),
        ];

        let rpc = MockRpc::new(replies);
        let cfg = ChainFeedConfig {
            poll_interval: Duration::from_millis(0),
            max_retries: 3,
            ..ChainFeedConfig::default()
        };
        let mut feed = ChainFeed::new(rpc, &["NVDA".to_owned()], cfg);
        let t = feed.next_tick().unwrap();
        assert_eq!(t.symbol, "NVDA");
    }
}
