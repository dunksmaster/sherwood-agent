//! A read-only ERC-20 view: metadata and balances, scaled to [`Decimal`].
//!
//! Amounts come back from the chain as base units (`wei`-equivalent). This
//! module scales them by `decimals()` so callers work in whole tokens. It never
//! writes — no `transfer`, no `approve`.

use crate::abi::{self};
use crate::keccak::selector;
use crate::{ChainError, EvmClient, Result};
use rust_decimal::Decimal;

/// Metadata read once and reused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenMeta {
    pub address: String,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
}

/// A handle to one ERC-20 contract on a given client.
pub struct Erc20<'a, C: ?Sized> {
    client: &'a C,
    address: String,
}

impl<'a, C: EvmClient + ?Sized> Erc20<'a, C> {
    /// Bind to `address` (checked to be a 20-byte hex string).
    pub fn new(client: &'a C, address: &str) -> Result<Self> {
        abi::address_word(address)?; // validate shape
        Ok(Self {
            client,
            address: address.to_lowercase(),
        })
    }

    /// The contract address, `0x`-prefixed lowercase.
    #[must_use]
    pub fn address(&self) -> &str {
        &self.address
    }

    async fn call0(&self, sig: &str) -> Result<Vec<u8>> {
        self.client
            .call(&self.address, &abi::calldata(selector(sig), &[]))
            .await
    }

    /// `decimals()`.
    pub async fn decimals(&self) -> Result<u8> {
        let v = abi::decode_u128(&self.call0("decimals()").await?)?;
        u8::try_from(v).map_err(|_| ChainError::Decode(format!("decimals() = {v}, out of range")))
    }

    /// `name()`.
    pub async fn name(&self) -> Result<String> {
        abi::decode_string(&self.call0("name()").await?)
    }

    /// `symbol()`.
    pub async fn symbol(&self) -> Result<String> {
        abi::decode_string(&self.call0("symbol()").await?)
    }

    /// `name()`, `symbol()`, `decimals()` together.
    pub async fn meta(&self) -> Result<TokenMeta> {
        Ok(TokenMeta {
            address: self.address.clone(),
            name: self.name().await?,
            symbol: self.symbol().await?,
            decimals: self.decimals().await?,
        })
    }

    /// `totalSupply()`, in base units.
    pub async fn total_supply_raw(&self) -> Result<u128> {
        abi::decode_u128(&self.call0("totalSupply()").await?)
    }

    /// `balanceOf(account)`, in base units.
    pub async fn balance_of_raw(&self, account: &str) -> Result<u128> {
        let data = abi::calldata(
            selector("balanceOf(address)"),
            &[abi::address_word(account)?],
        );
        abi::decode_u128(&self.client.call(&self.address, &data).await?)
    }

    /// `totalSupply()` scaled to whole tokens.
    pub async fn total_supply(&self, decimals: u8) -> Result<Decimal> {
        scale(self.total_supply_raw().await?, decimals)
    }

    /// `balanceOf(account)` scaled to whole tokens.
    pub async fn balance_of(&self, account: &str, decimals: u8) -> Result<Decimal> {
        scale(self.balance_of_raw(account).await?, decimals)
    }
}

/// Scale a base-unit amount to whole tokens. `Decimal` holds a 96-bit mantissa
/// (~7.9e28), so an implausibly large amount is an error rather than a silent
/// wrap. Transfer amounts are never taken from here — they stay in base units.
pub fn scale(raw: u128, decimals: u8) -> Result<Decimal> {
    let mantissa =
        i128::try_from(raw).map_err(|_| ChainError::Decode("amount exceeds i128".into()))?;
    Decimal::try_from_i128_with_scale(mantissa, u32::from(decimals).min(28))
        .map(|d| d.normalize())
        .map_err(|e| ChainError::Decode(format!("amount does not fit Decimal: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::MockRpc;
    use rust_decimal_macros::dec;
    use serde_json::json;

    fn word(hex: &str) -> serde_json::Value {
        json!(format!("0x{:0>64}", hex))
    }

    #[tokio::test]
    async fn reads_and_scales_metadata_and_balance() {
        let rpc = MockRpc::new(vec![
            // decimals() = 18
            Ok(word("12")),
            // balanceOf(x) = 1_500_000_000_000_000_000 (1.5 tokens)
            Ok(word("14d1120d7b160000")),
        ]);
        let t = Erc20::new(&rpc, "0x0bd7d308f8e1639fab988df18a8011f41eacad73").unwrap();
        let dec = t.decimals().await.unwrap();
        assert_eq!(dec, 18);
        assert_eq!(
            t.balance_of("0x1111111111111111111111111111111111111111", dec)
                .await
                .unwrap(),
            dec!(1.5)
        );
        assert_eq!(rpc.methods(), vec!["eth_call", "eth_call"]);
    }

    #[tokio::test]
    async fn decodes_abi_string_name() {
        let rpc = MockRpc::new(vec![Ok(json!(
            "0x0000000000000000000000000000000000000000000000000000000000000020\
               0000000000000000000000000000000000000000000000000000000000000004\
               4e56444100000000000000000000000000000000000000000000000000000000"
        ))]);
        let t = Erc20::new(&rpc, "0x0bd7d308f8e1639fab988df18a8011f41eacad73").unwrap();
        assert_eq!(t.symbol().await.unwrap(), "NVDA");
    }

    #[test]
    fn scale_is_exact_for_common_cases() {
        assert_eq!(scale(0, 18).unwrap(), dec!(0));
        assert_eq!(scale(1_000_000_000_000_000_000, 18).unwrap(), dec!(1));
        assert_eq!(scale(1_000_000, 6).unwrap(), dec!(1));
        assert_eq!(scale(2_500_000, 6).unwrap(), dec!(2.5));
    }
}
