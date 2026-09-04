//! Build the `UniversalRouter.execute(bytes,bytes[],uint256)` calldata for a
//! single-hop exact-input `V4_SWAP`.
//!
//! Every constant here is sourced, not remembered: command/action byte
//! values from `Commands.sol` / `Actions.sol`, the `ExactInputSingleParams`
//! field order from `IV4Router.sol`, and the decode shapes from
//! `V4Router.sol` / `CalldataDecoder.sol` — all in `Uniswap/universal-router`
//! and `Uniswap/v4-periphery` on GitHub. `minHopPriceX36 = 0` disables that
//! (newer, optional) per-hop price check; `amountOutMinimum` is this swap's
//! real slippage bound. Only ERC-20/ERC-20 swaps: no native-ETH leg, no
//! hook data (matches every pool this codebase has found so far — see
//! `docs/ROBINHOOD-CHAIN.md`).

use crate::abi_dyn::{
    encode_bytes_and_bytes_array, encode_bytes_array, encode_single_dynamic_struct, word_bool,
    word_uint,
};
use crate::DexError;
use sherwood_chain::abi;
use sherwood_chain::keccak::selector;
use sherwood_chain::univ4::PoolKey;

/// `Commands.sol`
const CMD_V4_SWAP: u8 = 0x10;
/// `Actions.sol`
const ACTION_SWAP_EXACT_IN_SINGLE: u8 = 0x06;
const ACTION_SETTLE_ALL: u8 = 0x0c;
const ACTION_TAKE_ALL: u8 = 0x0f;

/// A fully-specified single-hop exact-input swap.
#[derive(Debug, Clone)]
pub struct ExactInputSingleSwap {
    pub pool: PoolKey,
    /// `true` if the input token is `pool.currency0` (the swap direction).
    pub zero_for_one: bool,
    /// Raw base units of the input token.
    pub amount_in: u128,
    /// Raw base units — the real slippage bound for this swap.
    pub amount_out_minimum: u128,
}

impl ExactInputSingleSwap {
    /// The token being spent.
    #[must_use]
    pub fn token_in(&self) -> &str {
        if self.zero_for_one {
            &self.pool.currency0
        } else {
            &self.pool.currency1
        }
    }

    /// The token being received.
    #[must_use]
    pub fn token_out(&self) -> &str {
        if self.zero_for_one {
            &self.pool.currency1
        } else {
            &self.pool.currency0
        }
    }

    fn exact_input_single_params_body(&self) -> Result<Vec<u8>, DexError> {
        let mut body = Vec::with_capacity(352);
        body.extend_from_slice(&abi::address_word(&self.pool.currency0)?);
        body.extend_from_slice(&abi::address_word(&self.pool.currency1)?);
        body.extend_from_slice(&abi::uint_word(u128::from(self.pool.fee)));
        body.extend_from_slice(&abi::int_word(i128::from(self.pool.tick_spacing)));
        body.extend_from_slice(&abi::address_word(&self.pool.hooks)?);
        body.extend_from_slice(&word_bool(self.zero_for_one));
        body.extend_from_slice(&word_uint(self.amount_in));
        body.extend_from_slice(&word_uint(self.amount_out_minimum));
        body.extend_from_slice(&word_uint(0)); // minHopPriceX36 = 0 (disabled)
        body.extend_from_slice(&word_uint(9 * 32)); // hookData offset, relative to struct start
        body.extend_from_slice(&word_uint(0)); // hookData length = 0 (no hook data)
        Ok(body) // 10 head words + 1 tail word = 352 bytes = 0x160
    }

    fn settle_all_params(&self) -> Result<Vec<u8>, DexError> {
        let mut p = Vec::with_capacity(64);
        p.extend_from_slice(&abi::address_word(self.token_in())?);
        p.extend_from_slice(&word_uint(self.amount_in));
        Ok(p)
    }

    fn take_all_params(&self) -> Result<Vec<u8>, DexError> {
        let mut p = Vec::with_capacity(64);
        p.extend_from_slice(&abi::address_word(self.token_out())?);
        p.extend_from_slice(&word_uint(self.amount_out_minimum));
        Ok(p)
    }

    /// The `V4_SWAP` command's single `inputs[]` entry:
    /// `abi.encode(actions, params)`.
    fn v4_swap_input(&self) -> Result<Vec<u8>, DexError> {
        let actions = [
            ACTION_SWAP_EXACT_IN_SINGLE,
            ACTION_SETTLE_ALL,
            ACTION_TAKE_ALL,
        ];
        let params = vec![
            encode_single_dynamic_struct(&self.exact_input_single_params_body()?),
            self.settle_all_params()?,
            self.take_all_params()?,
        ];
        Ok(encode_bytes_and_bytes_array(&actions, &params))
    }

    /// The full `execute(bytes,bytes[],uint256)` calldata, `deadline` a unix
    /// timestamp. This is calldata only — building an `Eip1559Tx` from it
    /// (to, value=0, this data) and signing it is the caller's job; nothing
    /// here sends anything.
    pub fn execute_calldata(&self, deadline: u64) -> Result<Vec<u8>, DexError> {
        let commands = [CMD_V4_SWAP];
        let inputs = vec![self.v4_swap_input()?];

        let mut head_and_tail = Vec::new();
        head_and_tail.extend_from_slice(&word_uint(3 * 32)); // offset to commands
        let enc_commands = crate::abi_dyn::encode_bytes(&commands);
        head_and_tail.extend_from_slice(&word_uint((3 * 32 + enc_commands.len()) as u128)); // offset to inputs
        head_and_tail.extend_from_slice(&word_uint(deadline as u128));
        head_and_tail.extend_from_slice(&enc_commands);
        head_and_tail.extend_from_slice(&encode_bytes_array(&inputs));

        let mut out = Vec::with_capacity(4 + head_and_tail.len());
        out.extend_from_slice(&selector("execute(bytes,bytes[],uint256)"));
        out.extend_from_slice(&head_and_tail);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool() -> PoolKey {
        PoolKey::new(
            "0x5fc5360d0400a0fd4f2af552add042d716f1d168", // USDG
            "0xd0601ce157db5bdc3162bbac2a2c8af5320d9eec", // NVDA
            3000,
            60,
            "0x0000000000000000000000000000000000000000",
        )
        .unwrap()
    }

    fn swap() -> ExactInputSingleSwap {
        ExactInputSingleSwap {
            pool: pool(),
            zero_for_one: true,     // spending USDG (currency0) for NVDA
            amount_in: 200_000_000, // 200 USDG (6dp)
            amount_out_minimum: 900_000_000_000_000_000, // 0.9 NVDA (18dp), example bound
        }
    }

    #[test]
    fn token_in_and_out_follow_zero_for_one() {
        let s = swap();
        assert_eq!(s.token_in(), s.pool.currency0);
        assert_eq!(s.token_out(), s.pool.currency1);
    }

    #[test]
    fn exact_input_single_params_body_is_exactly_0x160_bytes() {
        // Matches CalldataDecoder's `lt(params.length, 0x160)` minimum-length
        // check for decodeSwapExactInSingleParams with empty hookData.
        let s = swap();
        let body = s.exact_input_single_params_body().unwrap();
        assert_eq!(body.len(), 0x160);
    }

    #[test]
    fn settle_all_and_take_all_params_are_exactly_0x40_bytes() {
        // Matches decodeCurrencyAndUint256's `lt(params.length, 0x40)` check.
        let s = swap();
        assert_eq!(s.settle_all_params().unwrap().len(), 0x40);
        assert_eq!(s.take_all_params().unwrap().len(), 0x40);
    }

    #[test]
    fn settle_all_params_uses_the_input_token_and_amount() {
        let s = swap();
        let p = s.settle_all_params().unwrap();
        assert_eq!(abi::to_hex(&p[12..32]), s.pool.currency0);
        assert_eq!(abi::decode_u128(&p[32..64]).unwrap(), s.amount_in);
    }

    #[test]
    fn take_all_params_uses_the_output_token_and_minimum() {
        let s = swap();
        let p = s.take_all_params().unwrap();
        assert_eq!(abi::to_hex(&p[12..32]), s.pool.currency1);
        assert_eq!(abi::decode_u128(&p[32..64]).unwrap(), s.amount_out_minimum);
    }

    #[test]
    fn execute_calldata_starts_with_the_execute_selector() {
        let s = swap();
        let cd = s.execute_calldata(1_800_000_000).unwrap();
        assert_eq!(&cd[0..4], &selector("execute(bytes,bytes[],uint256)"));
    }

    #[test]
    fn execute_calldata_embeds_the_deadline_in_the_head() {
        let s = swap();
        let deadline = 1_800_000_042u64;
        let cd = s.execute_calldata(deadline).unwrap();
        // head: selector(4) + offset_commands(32) + offset_inputs(32) + deadline(32)
        let deadline_word = &cd[4 + 64..4 + 96];
        assert_eq!(
            abi::decode_u128(deadline_word).unwrap(),
            u128::from(deadline)
        );
    }

    #[test]
    fn execute_calldata_v4_swap_command_and_action_bytes_are_present() {
        let s = swap();
        let cd = s.execute_calldata(1).unwrap();
        // commands (a `bytes` of length 1) sits right after the 3-word head:
        // selector(4) + head(96) + [len(32)=1][0x10 padded]
        let commands_len_word = &cd[4 + 96..4 + 96 + 32];
        assert_eq!(abi::decode_u128(commands_len_word).unwrap(), 1);
        assert_eq!(cd[4 + 96 + 32], CMD_V4_SWAP);
        // the three action bytes appear somewhere in the payload, in order.
        let hay = &cd[..];
        let needle = [
            ACTION_SWAP_EXACT_IN_SINGLE,
            ACTION_SETTLE_ALL,
            ACTION_TAKE_ALL,
        ];
        assert!(hay.windows(3).any(|w| w == needle));
    }
}
