//! Slippage and deadline bounds for a swap — pure math, no chain access.
//! Getting a *quote* (an expected output amount for a given input) is
//! `sherwood_chain::univ4`'s job (spot price × amount, or later a real
//! `V4Quoter` call); this module only turns that expectation into the two
//! numbers `sherwood-dex` actually needs to bound a swap: a minimum
//! acceptable output and a deadline.

use crate::DexError;
use std::time::{SystemTime, UNIX_EPOCH};

/// `amount_out_minimum = expected_amount_out * (10_000 - slippage_bps) / 10_000`.
/// `slippage_bps` in `0..10_000` (basis points; `50` = 0.50%).
pub fn amount_out_minimum(expected_amount_out: u128, slippage_bps: u32) -> Result<u128, DexError> {
    if slippage_bps >= 10_000 {
        return Err(DexError::InvalidSlippage(slippage_bps));
    }
    let keep = u128::from(10_000 - slippage_bps);
    let scaled = expected_amount_out
        .checked_mul(keep)
        .ok_or(DexError::Overflow)?;
    Ok(scaled / 10_000)
}

/// A unix-timestamp deadline `seconds_from_now` in the future.
pub fn deadline_from_now(seconds_from_now: u64) -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    now.saturating_add(seconds_from_now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_slippage_keeps_the_full_expected_amount() {
        assert_eq!(amount_out_minimum(1_000_000, 0).unwrap(), 1_000_000);
    }

    #[test]
    fn fifty_bps_shaves_half_a_percent() {
        // 1_000_000 * 9950 / 10000 = 995_000
        assert_eq!(amount_out_minimum(1_000_000, 50).unwrap(), 995_000);
    }

    #[test]
    fn one_hundred_percent_slippage_is_rejected() {
        assert!(amount_out_minimum(1_000_000, 10_000).is_err());
        assert!(amount_out_minimum(1_000_000, 10_001).is_err());
    }

    #[test]
    fn overflow_is_rejected_not_wrapped() {
        assert!(amount_out_minimum(u128::MAX, 1).is_err());
    }

    #[test]
    fn deadline_is_strictly_in_the_future() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let d = deadline_from_now(60);
        assert!(d >= now + 59 && d <= now + 61);
    }
}
