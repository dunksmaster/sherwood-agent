---
status: reference
last-updated: 2026-09-04
owner-step: v0.2
---

# Robinhood Chain — verified on-chain reference

Facts `sherwood-chain` and `sherwood-dex` build on, each checked against the
chain or the canonical Uniswap deployment file
([`Uniswap/contracts` `deployments/4663.md`](https://github.com/Uniswap/contracts/blob/main/deployments/4663.md)).
See [ADR-0006](adr/0006-robinhood-chain-venue.md).

## Network

| | |
|---|---|
| Chain ID | `4663` (`0x1237`) |
| RPC (public) | `https://rpc.mainnet.chain.robinhood.com` |
| Explorer | `https://robinhoodchain.blockscout.com` (Blockscout; Cloudflare-gated API) |
| Node | Arbitrum Nitro; `web3_clientVersion` → `nitro/…` |
| Gas token | ETH (no native token) |
| Log query limits | `eth_getLogs` caps at 10 000 results and times out on wide ranges — page by block window and filter by an indexed topic; expect `429` under bursts (back off) |

## Core tokens

| Symbol | Address | Decimals |
|---|---|---|
| WETH | `0x0Bd7D308f8E1639FAb988df18A8011f41EAcAD73` | 18 |
| USDG | `0x5fc5360D0400a0Fd4f2af552ADD042D716F1d168` | 6 |

## Stock Tokens (ERC-20)

Beacon proxies: each token is a ~283-byte proxy over one shared implementation
`0xb35490d6f9163de4f80d88dc75c3516eb64c5ae2` via beacon
`0xe10b6f6b275de231345c20d14ab812db62151b00`. **Upgradeable** — see ADR-0006's
caveats and the `chain-probe` pre-flight. All 18-decimal. `name()` reads
`"<Company> • Robinhood Token"`.

| Symbol | Address |
|---|---|
| NVDA | `0xd0601CE157Db5bdC3162BbaC2a2C8aF5320D9EEC` |
| TSLA | `0x322F0929c4625eD5bAd873c95208D54E1c003b2d` |
| AAPL | `0xaF3D76f1834A1d425780943C99Ea8A608f8a93f9` |
| MSFT | `0xe93237C50D904957Cf27E7B1133b510C669c2e74` |
| AMZN | `0x12f190a9F9d7D37a250758b26824B97CE941bF54` |
| GOOGL | `0x2e0847E8910a9732eB3fb1bb4b70a580ADAD4FE3` |
| META | `0xc0D6457C16Cc70d6790Dd43521C899C87ce02f35` |
| SPY | `0x117cc2133c37B721F49dE2A7a74833232B3B4C0C` |

(Cross-checked across two independent public write-ups. The chain's asset
registry is authoritative; treat this list as a convenience.)

## Uniswap deployment (chain 4663)

### v4

| Contract | Address |
|---|---|
| PoolManager | `0x8366a39cc670b4001a1121b8f6a443a643e40951` |
| PositionManager | `0x58daec3116aae6d93017baaea7749052e8a04fa7` |
| V4Quoter | `0x8dc178efb8111bb0973dd9d722ebeff267c98f94` |
| StateView | `0xf3334192d15450cdd385c8b70e03f9a6bd9e673b` |
| UniversalRouter | `0x8876789976decbfcbbbe364623c63652db8c0904` |
| Permit2 | `0x000000000022d473030f116ddee9f6b43ac78ba3` |

The PoolManager confirmed on-chain: it answers `extsload(bytes32)` (`0x1e2eaeaf`)
and `protocolFeeController()` and custodies multi-asset liquidity (~22k NVDA,
~1.7k WETH, ~48M USDG at time of writing).

### v3 / v2 (fallback venues)

| Contract | Address |
|---|---|
| UniswapV3Factory | `0x1f7d7550b1b028f7571e69a784071f0205fd2efa` |
| NonfungiblePositionManager (v3) | `0x73991a25c818bf1f1128deaab1492d45638de0d3` |
| QuoterV2 (v3) | `0x33e885ed0ec9bf04ecfb19341582aadcb4c8a9e7` |
| SwapRouter02 (v3) | `0xcaf681a66d020601342297493863e78c959e5cb2` |
| UniswapV2Factory | `0x8bceaa40b9acdfaedf85adf4ff01f5ad6517937f` |
| UniswapV2Router02 | `0x89e5db8b5aa49aa85ac63f691524311aeb649eba` |
| UniswapInterfaceMulticall | `0x282a3c4d320cc7f0d5eaf56b8029e4b88338f0a3` |

## Reading a v4 pool price

1. **PoolKey** = `(address currency0, address currency1, uint24 fee, int24 tickSpacing, address hooks)`
   with `currency0 < currency1` by address.
2. **PoolId** = `keccak256(abi.encode(poolKey))` — five left-padded 32-byte words.
3. **`StateView.getSlot0(bytes32 poolId)`** → `(uint160 sqrtPriceX96, int24 tick, uint24 protocolFee, uint24 lpFee)`.
   **`StateView.getLiquidity(bytes32 poolId)`** → `uint128`.
4. **Price** (currency1 per currency0, raw units) = `(sqrtPriceX96 / 2**96) ** 2`.
   Human price = raw × `10**(decimals0 - decimals1)`. Invert for the other
   direction.
5. **Pool discovery:** `PoolManager` emits
   `Initialize` — topic0 `0xdd466e674ea557f56295e2d0218a125ea4b4f0f6f3307b95f85e6110838d6438`,
   with `id`, `currency0`, `currency1` indexed. Filter by an indexed currency to
   list a token's pools, then rank by `getLiquidity`.

### Worked example — an NVDA/USDG pool

From an `Initialize` log on the PoolManager:

| field | value |
|---|---|
| poolId | `0xd80b3b4cc602181da7004070f75c54b3107807f0cc71072d8c373662d44df972` |
| currency0 | `0x5fc5…d168` (USDG, 6 dp) |
| currency1 | `0xd060…9eec` (NVDA, 18 dp) |
| fee | `900000` |
| tickSpacing | `18000` |
| hooks | `0x0` (none) |

`sqrtPriceX96 = 5614838607502676772851347767430022` →
`(sqrtP/2^96)^2 ≈ 5.02e9` (NVDA-raw per USDG-raw) → invert and apply
`10^(18-6)` → **≈ 199 USDG per NVDA** — a sane NVDA share price, so the math and
the decimal handling check out.

**Note:** more than one NVDA/USDG pool exists (different `fee`/`tickSpacing`;
some are launchpad/full-range bootstrap pools). `sherwood-chain` must enumerate
them and select by liquidity, not take the first `Initialize` it sees.
