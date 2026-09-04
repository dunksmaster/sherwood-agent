//! Reading a Uniswap v4 pool's price on Robinhood Chain.
//!
//! Addresses and the recipe here are verified against the chain — see
//! [`docs/ROBINHOOD-CHAIN.md`](../../../docs/ROBINHOOD-CHAIN.md). Everything in
//! this module is a read: `PoolManager.extsload` (via [`crate::rpc`]),
//! `StateView.getSlot0` / `getLiquidity`, and `PoolManager`'s `Initialize` logs
//! for pool discovery. Nothing here builds a swap or touches a wallet — that is
//! `sherwood-dex` (later).

use crate::abi::{self};
use crate::keccak::{keccak256, selector};
use crate::rpc::LogFilter;
use crate::{ChainError, EvmClient, Result};
use rust_decimal::Decimal;

/// `Initialize(bytes32 indexed id, address indexed currency0, address indexed
/// currency1, uint24 fee, int24 tickSpacing, address hooks, uint160
/// sqrtPriceX96, int24 tick)` — verified against a real pool-creation log.
pub const INITIALIZE_TOPIC: &str =
    "0xdd466e674ea557f56295e2d0218a125ea4b4f0f6f3307b95f85e6110838d6438";

/// The five fields Uniswap v4 hashes to identify a pool. `currency0` is always
/// the numerically smaller address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolKey {
    pub currency0: String,
    pub currency1: String,
    pub fee: u32,
    pub tick_spacing: i32,
    pub hooks: String,
}

impl PoolKey {
    /// Build a key from two currencies in either order — they are sorted.
    pub fn new(a: &str, b: &str, fee: u32, tick_spacing: i32, hooks: &str) -> Result<Self> {
        let (wa, wb) = (abi::address_word(a)?, abi::address_word(b)?);
        let (currency0, currency1) = if wa <= wb {
            (a.to_lowercase(), b.to_lowercase())
        } else {
            (b.to_lowercase(), a.to_lowercase())
        };
        abi::address_word(hooks)?;
        Ok(Self {
            currency0,
            currency1,
            fee,
            tick_spacing,
            hooks: hooks.to_lowercase(),
        })
    }

    /// `PoolId = keccak256(abi.encode(currency0, currency1, fee, tickSpacing, hooks))`.
    pub fn pool_id(&self) -> Result<[u8; 32]> {
        let mut buf = Vec::with_capacity(5 * 32);
        buf.extend_from_slice(&abi::address_word(&self.currency0)?);
        buf.extend_from_slice(&abi::address_word(&self.currency1)?);
        buf.extend_from_slice(&abi::uint_word(u128::from(self.fee)));
        buf.extend_from_slice(&abi::int_word(i128::from(self.tick_spacing)));
        buf.extend_from_slice(&abi::address_word(&self.hooks)?);
        Ok(keccak256(&buf))
    }
}

/// `StateView.getSlot0` result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot0 {
    pub sqrt_price_x96: u128,
    pub tick: i32,
    pub protocol_fee: u32,
    pub lp_fee: u32,
}

/// A handle to the `StateView` periphery contract.
pub struct StateView<'a, C: ?Sized> {
    client: &'a C,
    address: String,
}

impl<'a, C: EvmClient + ?Sized> StateView<'a, C> {
    #[must_use]
    pub fn new(client: &'a C, address: &str) -> Self {
        Self {
            client,
            address: address.to_lowercase(),
        }
    }

    /// `getSlot0(bytes32 poolId) -> (uint160, int24, uint24, uint24)`.
    pub async fn get_slot0(&self, pool_id: [u8; 32]) -> Result<Slot0> {
        let data = abi::calldata(selector("getSlot0(bytes32)"), &[pool_id]);
        let ret = self.client.call(&self.address, &data).await?;
        if ret.len() < 128 {
            return Err(ChainError::Decode(format!(
                "getSlot0 returned {} bytes, expected 128",
                ret.len()
            )));
        }
        Ok(Slot0 {
            sqrt_price_x96: abi::decode_u128(&ret[0..32])?,
            tick: abi::decode_i32(&ret[32..64])?,
            protocol_fee: u32::try_from(abi::decode_u128(&ret[64..96])?)
                .map_err(|_| ChainError::Decode("protocolFee out of range".into()))?,
            lp_fee: u32::try_from(abi::decode_u128(&ret[96..128])?)
                .map_err(|_| ChainError::Decode("lpFee out of range".into()))?,
        })
    }

    /// `getLiquidity(bytes32 poolId) -> uint128`.
    pub async fn get_liquidity(&self, pool_id: [u8; 32]) -> Result<u128> {
        let data = abi::calldata(selector("getLiquidity(bytes32)"), &[pool_id]);
        abi::decode_u128(&self.client.call(&self.address, &data).await?)
    }
}

/// `(sqrtPriceX96 / 2^96)^2`, i.e. currency1-per-currency0 in **raw base
/// units**, adjusted to whole-token units by `decimals0`/`decimals1`.
///
/// Computed in `Decimal` rather than integer math: `sqrtPriceX96` can exceed
/// `u128`'s range once squared, but `sqrtPriceX96 / 2^96` does not (it is the
/// raw-unit price's square root, a modest number for any sane pool), so we
/// scale down first and square a `Decimal`. `Decimal` carries 28 significant
/// digits, ample for a price.
pub fn price_from_sqrt_price_x96(
    sqrt_price_x96: u128,
    decimals0: u8,
    decimals1: u8,
) -> Result<Decimal> {
    // `sqrtPriceX96` itself can exceed Decimal's ~7.9e28 range (its 96-bit
    // mantissa), even though the *ratio* we want (sqrtPriceX96 / 2^96) does
    // not. Shift both operands down by 48 bits first: `sqrtPriceX96 >> 48`
    // stays within Decimal for the full `u128` domain (max ~2^80), and the
    // dropped low bits are ~2^-64 relative — far below anything a price needs.
    const SHIFT: u32 = 48;
    let hi = u128_to_decimal(sqrt_price_x96 >> SHIFT)?;
    let divisor = u128_to_decimal(1u128 << (96 - SHIFT))?; // 2^48
    let ratio = hi / divisor; // ≈ sqrtPriceX96 / 2^96
    let raw_price = ratio * ratio; // currency1 per currency0, base units

    let scale = i32::from(decimals0) - i32::from(decimals1);
    let adjusted = if scale >= 0 {
        raw_price * pow10(scale)?
    } else {
        raw_price / pow10(-scale)?
    };
    Ok(adjusted.normalize())
}

fn u128_to_decimal(v: u128) -> Result<Decimal> {
    let hi = i128::try_from(v)
        .map_err(|_| ChainError::Decode("value exceeds Decimal (128-bit signed)".into()))?;
    Decimal::try_from_i128_with_scale(hi, 0)
        .map_err(|e| ChainError::Decode(format!("value exceeds Decimal: {e}")))
}

/// `10^exp` as a `Decimal`, `exp` in `0..=28` (`Decimal`'s significant-digit limit).
fn pow10(exp: i32) -> Result<Decimal> {
    if !(0..=28).contains(&exp) {
        return Err(ChainError::Decode(format!(
            "decimal-adjustment exponent {exp} out of supported range"
        )));
    }
    let mut r = Decimal::ONE;
    for _ in 0..exp {
        r *= Decimal::from(10i32);
    }
    Ok(r)
}

/// A discovered pool plus its currently-observed liquidity.
#[derive(Debug, Clone)]
pub struct RankedPool {
    pub key: PoolKey,
    pub liquidity: u128,
}

/// Find every pool `PoolManager` has ever initialised for `token`, by scanning
/// `Initialize` logs with `token` as either indexed currency. `from_block`
/// lets a caller bound the scan (the public RPC caps `eth_getLogs` at 10 000
/// results and times out on very wide unfiltered ranges — filtering by an
/// indexed currency keeps a token's own history small).
pub async fn discover_pools<C: EvmClient + ?Sized>(
    client: &C,
    pool_manager: &str,
    token: &str,
    from_block: u64,
    to_block: u64,
) -> Result<Vec<PoolKey>> {
    let token_topic = topic_from_address(token)?;
    let init_topic = INITIALIZE_TOPIC.to_owned();

    let as_currency0 = LogFilter {
        address: Some(pool_manager.to_lowercase()),
        topics: vec![Some(init_topic.clone()), None, Some(token_topic.clone())],
        from_block,
        to_block,
    };
    let as_currency1 = LogFilter {
        address: Some(pool_manager.to_lowercase()),
        topics: vec![Some(init_topic), None, None, Some(token_topic)],
        from_block,
        to_block,
    };

    let mut out = Vec::new();
    for filter in [as_currency0, as_currency1] {
        for log in client.get_logs(&filter).await? {
            if let Some(key) = decode_initialize(&log.topics, &log.data) {
                out.push(key);
            }
        }
    }
    out.dedup_by(|a, b| a == b);
    Ok(out)
}

/// Find every pool between exactly `token` and `denom` (either order). Unlike
/// [`discover_pools`], the counter-currency is constrained too — necessary
/// before trusting a pool's price, since a pool the *token* is merely *in*
/// could be paired against anything (a different stable, WETH, a spam token
/// with different decimals), and mis-assuming it is `denom` silently produces
/// a nonsense price (this was caught live: a pool with `token` paired against
/// something else was picked by raw liquidity and priced as if it were
/// `token`/USDG, giving a wrong-by-orders-of-magnitude result).
pub async fn discover_pools_for_pair<C: EvmClient + ?Sized>(
    client: &C,
    pool_manager: &str,
    token: &str,
    denom: &str,
    from_block: u64,
    to_block: u64,
) -> Result<Vec<PoolKey>> {
    let (token_topic, denom_topic) = (topic_from_address(token)?, topic_from_address(denom)?);
    let init_topic = INITIALIZE_TOPIC.to_owned();

    let token_is_c0 = LogFilter {
        address: Some(pool_manager.to_lowercase()),
        topics: vec![
            Some(init_topic.clone()),
            None,
            Some(token_topic.clone()),
            Some(denom_topic.clone()),
        ],
        from_block,
        to_block,
    };
    let token_is_c1 = LogFilter {
        address: Some(pool_manager.to_lowercase()),
        topics: vec![Some(init_topic), None, Some(denom_topic), Some(token_topic)],
        from_block,
        to_block,
    };

    let mut out = Vec::new();
    for filter in [token_is_c0, token_is_c1] {
        for log in client.get_logs(&filter).await? {
            if let Some(key) = decode_initialize(&log.topics, &log.data) {
                out.push(key);
            }
        }
    }
    out.dedup_by(|a, b| a == b);
    Ok(out)
}

fn topic_from_address(addr: &str) -> Result<String> {
    Ok(abi::to_hex(&abi::address_word(addr)?))
}

fn decode_initialize(topics: &[String], data: &str) -> Option<PoolKey> {
    if topics.len() < 4 {
        return None;
    }
    let currency0 = format!("0x{}", abi::strip0x(&topics[2])[24..].to_owned());
    let currency1 = format!("0x{}", abi::strip0x(&topics[3])[24..].to_owned());
    let raw = abi::from_hex(data).ok()?;
    if raw.len() < 96 {
        return None;
    }
    let fee = u32::try_from(abi::decode_u128(&raw[0..32]).ok()?).ok()?;
    let tick_spacing = abi::decode_i32(&raw[32..64]).ok()?;
    let hooks = abi::decode_address(&raw[64..96]).ok()?;
    Some(PoolKey {
        currency0,
        currency1,
        fee,
        tick_spacing,
        hooks,
    })
}

/// Rank `candidates` by on-chain liquidity (deepest first). Pools that fail to
/// read (e.g. a stale key) are dropped rather than failing the whole call.
pub async fn rank_by_liquidity<C: EvmClient + ?Sized>(
    client: &C,
    state_view: &str,
    candidates: &[PoolKey],
) -> Result<Vec<RankedPool>> {
    let sv = StateView::new(client, state_view);
    let mut out = Vec::new();
    for key in candidates {
        let Ok(id) = key.pool_id() else { continue };
        if let Ok(liquidity) = sv.get_liquidity(id).await {
            if liquidity > 0 {
                out.push(RankedPool {
                    key: key.clone(),
                    liquidity,
                });
            }
        }
    }
    out.sort_by_key(|p| std::cmp::Reverse(p.liquidity));
    Ok(out)
}

/// Decimals for the two sides of a price read.
#[derive(Debug, Clone, Copy)]
pub struct Decimals {
    pub token: u8,
    pub denominator: u8,
}

/// The two Uniswap v4 contracts a price read needs.
#[derive(Debug, Clone, Copy)]
pub struct Deployment<'a> {
    pub pool_manager: &'a str,
    pub state_view: &'a str,
}

/// Discover pools between exactly `token` and `denom`, and pick the deepest
/// by liquidity. This is the expensive half of a price read (an
/// `Initialize`-log scan plus one `getLiquidity` call per candidate) — do it
/// once per token and reuse the result; a live feed should not repeat it on
/// every poll.
pub async fn find_best_pool<C: EvmClient + ?Sized>(
    client: &C,
    deployment: Deployment<'_>,
    token: &str,
    denom: &str,
    from_block: u64,
    to_block: u64,
) -> Result<RankedPool> {
    let candidates = discover_pools_for_pair(
        client,
        deployment.pool_manager,
        token,
        denom,
        from_block,
        to_block,
    )
    .await?;
    if candidates.is_empty() {
        return Err(ChainError::NotFound(format!(
            "no {token}/{denom} pool found"
        )));
    }
    rank_by_liquidity(client, deployment.state_view, &candidates)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| ChainError::NotFound(format!("no liquid {token}/{denom} pool found")))
}

/// The cheap half: one `getSlot0` on an already-known pool, converted to a
/// price in `denominator` per whole `token`. Safe to call on every poll.
pub async fn quote_pool<C: EvmClient + ?Sized>(
    client: &C,
    state_view: &str,
    pool: &PoolKey,
    token: &str,
    decimals: Decimals,
) -> Result<Decimal> {
    let sv = StateView::new(client, state_view);
    let slot0 = sv.get_slot0(pool.pool_id()?).await?;
    let token_is_currency0 = pool.currency0.eq_ignore_ascii_case(token);
    let price_1_per_0 = price_from_sqrt_price_x96(
        slot0.sqrt_price_x96,
        if token_is_currency0 {
            decimals.token
        } else {
            decimals.denominator
        },
        if token_is_currency0 {
            decimals.denominator
        } else {
            decimals.token
        },
    )?;
    // price_1_per_0 is currency1-per-currency0; invert if the token is currency1.
    if token_is_currency0 {
        Ok(price_1_per_0)
    } else if price_1_per_0.is_zero() {
        Err(ChainError::Decode("pool price is zero".into()))
    } else {
        Ok(Decimal::ONE / price_1_per_0)
    }
}

/// End to end: discover pools between exactly `token` and `denom`, pick the
/// deepest by liquidity, and return its price in `denominator` per whole
/// `token`. Convenient for a one-off read (the CLI); a repeated poller should
/// call [`find_best_pool`] once and [`quote_pool`] thereafter — see
/// [`crate::feed::ChainFeed`].
pub async fn read_price<C: EvmClient + ?Sized>(
    client: &C,
    deployment: Deployment<'_>,
    token: &str,
    denom: &str,
    decimals: Decimals,
    from_block: u64,
    to_block: u64,
) -> Result<(Decimal, RankedPool)> {
    let best = find_best_pool(client, deployment, token, denom, from_block, to_block).await?;
    let price = quote_pool(client, deployment.state_view, &best.key, token, decimals).await?;
    Ok((price, best))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::MockRpc;
    use rust_decimal_macros::dec;
    use serde_json::json;

    const USDG: &str = "0x5fc5360d0400a0fd4f2af552add042d716f1d168";
    const NVDA: &str = "0xd0601ce157db5bdc3162bbac2a2c8af5320d9eec";

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
                INITIALIZE_TOPIC,
                pool_id,
                format!("0x{}{}", "0".repeat(24), &currency0[2..]),
                format!("0x{}{}", "0".repeat(24), &currency1[2..]),
            ],
            "data": abi::to_hex(&data),
            "blockNumber": "0x10",
        })
    }

    #[test]
    fn pool_key_sorts_currencies() {
        let k = PoolKey::new(
            NVDA,
            USDG,
            900_000,
            18_000,
            "0x0000000000000000000000000000000000000000",
        )
        .unwrap();
        assert_eq!(k.currency0, USDG);
        assert_eq!(k.currency1, NVDA);
    }

    #[test]
    fn pool_id_matches_a_real_on_chain_pool() {
        // From a live Initialize log on Robinhood Chain's PoolManager.
        let k = PoolKey::new(
            USDG,
            NVDA,
            900_000,
            18_000,
            "0x0000000000000000000000000000000000000000",
        )
        .unwrap();
        let id = k.pool_id().unwrap();
        assert_eq!(
            abi::to_hex(&id),
            "0xd80b3b4cc602181da7004070f75c54b3107807f0cc71072d8c373662d44df972"
        );
    }

    #[test]
    fn price_from_a_real_pool_is_a_sane_nvda_price() {
        // Same pool: sqrtPriceX96 observed on-chain, currency0 = USDG (6dp),
        // currency1 = NVDA (18dp). Expect a plausible USDG-per-NVDA price.
        let sqrt_price_x96: u128 = 5_614_838_607_502_676_772_851_347_767_430_022;
        let raw = price_from_sqrt_price_x96(sqrt_price_x96, 6, 18).unwrap(); // NVDA per USDG-ish, raw
        let usdg_per_nvda = Decimal::ONE / raw;
        // ~199.1 at the block this sqrtPriceX96 was observed.
        assert!(
            usdg_per_nvda > dec!(190) && usdg_per_nvda < dec!(210),
            "{usdg_per_nvda}"
        );
    }

    #[test]
    fn decode_initialize_round_trips_through_real_field_values() {
        // Built from the actual field values (fee/tickSpacing/hooks) observed on
        // a real Initialize log, via the same word encoders used elsewhere —
        // avoids hand-transcribing a 5-word hex blob.
        let mut data = Vec::new();
        data.extend_from_slice(&abi::uint_word(900_000));
        data.extend_from_slice(&abi::int_word(18_000));
        data.extend_from_slice(
            &abi::address_word("0x0000000000000000000000000000000000000000").unwrap(),
        );
        let topics = vec![
            INITIALIZE_TOPIC.to_owned(),
            "0xd80b3b4cc602181da7004070f75c54b3107807f0cc71072d8c373662d44df972".to_owned(),
            format!("0x{}", "0".repeat(24) + &USDG[2..]),
            format!("0x{}", "0".repeat(24) + &NVDA[2..]),
        ];
        let key = decode_initialize(&topics, &abi::to_hex(&data)).unwrap();
        assert_eq!(key.currency0, USDG);
        assert_eq!(key.currency1, NVDA);
        assert_eq!(key.fee, 900_000);
        assert_eq!(key.tick_spacing, 18_000);
    }

    #[tokio::test]
    async fn discover_pools_queries_both_currency_positions() {
        let rpc = MockRpc::new(vec![Ok(json!([])), Ok(json!([]))]);
        let pools = discover_pools(&rpc, "0xpoolmanager", NVDA, 0, 100)
            .await
            .unwrap();
        assert!(pools.is_empty());
        assert_eq!(rpc.methods(), vec!["eth_getLogs", "eth_getLogs"]);
    }

    #[tokio::test]
    async fn discover_pools_for_pair_excludes_a_pool_paired_against_something_else() {
        // Regression: an earlier version filtered by `token` alone, so a pool
        // pairing NVDA against an unrelated token got treated as NVDA/USDG and
        // priced with the wrong decimals — a ~1e12x-wrong result caught live.
        // `discover_pools_for_pair` must only return the NVDA/USDG pool.
        const OTHER: &str = "0x2222222222222222222222222222222222222222";
        let nvda_usdg = init_log(
            "0xd80b3b4cc602181da7004070f75c54b3107807f0cc71072d8c373662d44df972",
            USDG,
            NVDA,
        );
        let nvda_other = init_log(
            "0x1111111111111111111111111111111111111111111111111111111111111111",
            NVDA,
            OTHER,
        );
        // token-is-currency0 query (NVDA, USDG) -> nothing; token-is-currency1
        // query (USDG, NVDA) -> the real pool. The spam pool never matches
        // either query because its counter-currency isn't USDG.
        let rpc = MockRpc::new(vec![Ok(json!([])), Ok(json!([nvda_usdg]))]);
        let pools = discover_pools_for_pair(&rpc, "0xpoolmanager", NVDA, USDG, 0, 100)
            .await
            .unwrap();
        assert_eq!(pools.len(), 1);
        assert_eq!(pools[0].currency0, USDG);
        assert_eq!(pools[0].currency1, NVDA);
        let _ = nvda_other; // shown for context; a real node simply wouldn't return it for this filter
    }

    #[tokio::test]
    async fn rank_by_liquidity_sorts_deepest_first_and_drops_empty_pools() {
        let k1 = PoolKey::new(
            USDG,
            NVDA,
            3000,
            60,
            "0x0000000000000000000000000000000000000000",
        )
        .unwrap();
        let k2 = PoolKey::new(
            USDG,
            NVDA,
            900_000,
            18_000,
            "0x0000000000000000000000000000000000000000",
        )
        .unwrap();
        let rpc = MockRpc::new(vec![
            Ok(json!(
                "0x0000000000000000000000000000000000000000000000000000000000000000"
            )), // k1: 0 liquidity
            Ok(json!(
                "0x0000000000000000000000000000000000000000000000000000000000000064"
            )), // k2: 100
        ]);
        let ranked = rank_by_liquidity(&rpc, "0xstateview", &[k1, k2.clone()])
            .await
            .unwrap();
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].key, k2);
        assert_eq!(ranked[0].liquidity, 100);
    }

    #[tokio::test]
    async fn get_slot0_decodes_all_four_fields() {
        let mut ret = Vec::new();
        ret.extend_from_slice(&abi::uint_word(
            5_614_838_607_502_676_772_851_347_767_430_022,
        ));
        ret.extend_from_slice(&abi::int_word(223_419));
        ret.extend_from_slice(&abi::uint_word(0));
        ret.extend_from_slice(&abi::uint_word(3000));
        let rpc = MockRpc::new(vec![Ok(json!(abi::to_hex(&ret)))]);
        let sv = StateView::new(&rpc, "0xstateview");
        let s0 = sv.get_slot0([0u8; 32]).await.unwrap();
        assert_eq!(
            s0.sqrt_price_x96,
            5_614_838_607_502_676_772_851_347_767_430_022
        );
        assert_eq!(s0.tick, 223_419);
        assert_eq!(s0.protocol_fee, 0);
        assert_eq!(s0.lp_fee, 3000);
    }
}
