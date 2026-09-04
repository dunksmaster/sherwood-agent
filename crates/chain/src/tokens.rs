//! Known Robinhood Chain addresses — Stock Tokens, stables, and the Uniswap
//! v4 deployment. Verified against the chain; see
//! [`docs/ROBINHOOD-CHAIN.md`](../../../docs/ROBINHOOD-CHAIN.md). A
//! convenience for CLI commands and [`crate::feed::ChainFeed`], not
//! authoritative — the chain's own asset registry is.

/// One known ERC-20.
#[derive(Debug, Clone, Copy)]
pub struct KnownToken {
    pub symbol: &'static str,
    pub address: &'static str,
    pub decimals: u8,
}

pub const KNOWN_TOKENS: &[KnownToken] = &[
    KnownToken {
        symbol: "NVDA",
        address: "0xd0601CE157Db5bdC3162BbaC2a2C8aF5320D9EEC",
        decimals: 18,
    },
    KnownToken {
        symbol: "TSLA",
        address: "0x322F0929c4625eD5bAd873c95208D54E1c003b2d",
        decimals: 18,
    },
    KnownToken {
        symbol: "AAPL",
        address: "0xaF3D76f1834A1d425780943C99Ea8A608f8a93f9",
        decimals: 18,
    },
    KnownToken {
        symbol: "MSFT",
        address: "0xe93237C50D904957Cf27E7B1133b510C669c2e74",
        decimals: 18,
    },
    KnownToken {
        symbol: "AMZN",
        address: "0x12f190a9F9d7D37a250758b26824B97CE941bF54",
        decimals: 18,
    },
    KnownToken {
        symbol: "GOOGL",
        address: "0x2e0847E8910a9732eB3fb1bb4b70a580ADAD4FE3",
        decimals: 18,
    },
    KnownToken {
        symbol: "META",
        address: "0xc0D6457C16Cc70d6790Dd43521C899C87ce02f35",
        decimals: 18,
    },
    KnownToken {
        symbol: "SPY",
        address: "0x117cc2133c37B721F49dE2A7a74833232B3B4C0C",
        decimals: 18,
    },
    KnownToken {
        symbol: "USDG",
        address: "0x5fc5360D0400a0Fd4f2af552ADD042D716F1d168",
        decimals: 6,
    },
    KnownToken {
        symbol: "WETH",
        address: "0x0Bd7D308f8E1639FAb988df18A8011f41EAcAD73",
        decimals: 18,
    },
];

/// The Uniswap v4 deployment on Robinhood Chain (chain id `4663`).
pub const POOL_MANAGER: &str = "0x8366a39cc670b4001a1121b8f6a443a643e40951";
pub const STATE_VIEW: &str = "0xf3334192d15450cdd385c8b70e03f9a6bd9e673b";

pub const DEFAULT_RPC: &str = "https://rpc.mainnet.chain.robinhood.com";

/// Resolve a known symbol (case-insensitive) or pass an address through
/// as-is. Returns `(symbol, address, decimals)`; an unknown symbol is assumed
/// to already be an address and defaults to 18 decimals.
#[must_use]
pub fn resolve(token: &str) -> (String, String, u8) {
    KNOWN_TOKENS
        .iter()
        .find(|t| t.symbol.eq_ignore_ascii_case(token))
        .map_or_else(
            || (token.to_owned(), token.to_owned(), 18),
            |t| (t.symbol.to_owned(), t.address.to_owned(), t.decimals),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_is_case_insensitive_for_known_symbols() {
        let (sym, addr, dec) = resolve("nvda");
        assert_eq!(sym, "NVDA");
        assert_eq!(addr, "0xd0601CE157Db5bdC3162BbaC2a2C8aF5320D9EEC");
        assert_eq!(dec, 18);
    }

    #[test]
    fn resolve_passes_through_an_unknown_address() {
        let (sym, addr, dec) = resolve("0xdeadbeef00000000000000000000000000dead");
        assert_eq!(sym, "0xdeadbeef00000000000000000000000000dead");
        assert_eq!(addr, "0xdeadbeef00000000000000000000000000dead");
        assert_eq!(dec, 18);
    }
}
