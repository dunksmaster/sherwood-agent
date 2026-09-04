//! Calldata for the two approvals a `V4_SWAP` needs before it can settle an
//! ERC-20 input: a standard `ERC20.approve` on the token naming Permit2 as
//! spender, then `Permit2.approve` naming the `UniversalRouter` as spender.
//! Neither is built into a signed or sent transaction here — calldata only.
//!
//! These helpers deliberately do **not** support an "unlimited" approval —
//! `amount` is bounded to `u128` (matching the swap amounts themselves,
//! which are `uint128` in the v4 router). Approve what a swap actually
//! needs, not `type(uint256).max`; a bounded approval bounds the blast
//! radius of anything that goes on to misuse it.

use crate::DexError;
use sherwood_chain::abi;
use sherwood_chain::keccak::selector;

/// The canonical Permit2 deployment address (same on every chain, including
/// Robinhood Chain — see `docs/ROBINHOOD-CHAIN.md`).
pub const PERMIT2_ADDRESS: &str = "0x000000000022d473030f116ddee9f6b43ac78ba3";

/// `ERC20.approve(spender, amount)` calldata.
pub fn erc20_approve_calldata(spender: &str, amount: u128) -> Result<Vec<u8>, DexError> {
    let mut data = Vec::with_capacity(4 + 64);
    data.extend_from_slice(&selector("approve(address,uint256)"));
    data.extend_from_slice(&abi::address_word(spender)?);
    data.extend_from_slice(&abi::uint_word(amount));
    Ok(data)
}

/// `Permit2.approve(token, spender, amount, expiration)` calldata.
/// `amount` (really a `uint160`) and `expiration` (really a `uint48`, a unix
/// timestamp) both encode the same as any smaller `uintN` — right-aligned in
/// their word, zero-padded on the left.
pub fn permit2_approve_calldata(
    token: &str,
    spender: &str,
    amount: u128,
    expiration_unix: u64,
) -> Result<Vec<u8>, DexError> {
    let mut data = Vec::with_capacity(4 + 128);
    data.extend_from_slice(&selector("approve(address,address,uint160,uint48)"));
    data.extend_from_slice(&abi::address_word(token)?);
    data.extend_from_slice(&abi::address_word(spender)?);
    data.extend_from_slice(&abi::uint_word(amount));
    data.extend_from_slice(&abi::uint_word(u128::from(expiration_unix)));
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sherwood_chain::abi::decode_u128;

    const TOKEN: &str = "0x5fc5360d0400a0fd4f2af552add042d716f1d168";
    const ROUTER: &str = "0x1111111111111111111111111111111111111111";

    #[test]
    fn erc20_approve_starts_with_the_known_selector() {
        let cd = erc20_approve_calldata(PERMIT2_ADDRESS, 1000).unwrap();
        assert_eq!(&cd[0..4], &[0x09, 0x5e, 0xa7, 0xb3]); // approve(address,uint256)
        assert_eq!(cd.len(), 4 + 64);
    }

    #[test]
    fn erc20_approve_encodes_spender_and_amount() {
        let cd = erc20_approve_calldata(PERMIT2_ADDRESS, 12345).unwrap();
        assert_eq!(abi::to_hex(&cd[16..36]), PERMIT2_ADDRESS);
        assert_eq!(decode_u128(&cd[36..68]).unwrap(), 12345);
    }

    #[test]
    fn permit2_approve_encodes_all_four_fields_in_order() {
        let cd = permit2_approve_calldata(TOKEN, ROUTER, 999, 1_800_000_000).unwrap();
        assert_eq!(cd.len(), 4 + 128);
        assert_eq!(abi::to_hex(&cd[16..36]), TOKEN);
        assert_eq!(abi::to_hex(&cd[48..68]), ROUTER);
        assert_eq!(decode_u128(&cd[68..100]).unwrap(), 999);
        assert_eq!(decode_u128(&cd[100..132]).unwrap(), 1_800_000_000);
    }
}
