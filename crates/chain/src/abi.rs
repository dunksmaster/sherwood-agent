//! The sliver of ABI coding sherwood needs for read calls: left-pad an address
//! or an integer into a 32-byte word, and decode a `uint256`, an `address`, or
//! a dynamic `string` back out. Not a general ABI codec — just the shapes used
//! by [`crate::erc20`] and [`crate::probe`].

use crate::{ChainError, Result};

/// A 32-byte ABI word.
pub type Word = [u8; 32];

/// Strip a leading `0x`/`0X` if present.
#[must_use]
pub fn strip0x(s: &str) -> &str {
    s.strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s)
}

/// Parse a hex string (with or without `0x`) into bytes.
pub fn from_hex(s: &str) -> Result<Vec<u8>> {
    hex::decode(strip0x(s)).map_err(|e| ChainError::Decode(format!("bad hex: {e}")))
}

/// `0x`-prefixed lowercase hex.
#[must_use]
pub fn to_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

/// Left-pad a 20-byte address into a 32-byte word. Accepts `0x`-prefixed or
/// bare, any case; rejects anything that is not exactly 20 bytes.
pub fn address_word(addr: &str) -> Result<Word> {
    let raw = from_hex(addr)?;
    if raw.len() != 20 {
        return Err(ChainError::Decode(format!(
            "address must be 20 bytes, got {}",
            raw.len()
        )));
    }
    let mut w = [0u8; 32];
    w[12..].copy_from_slice(&raw);
    Ok(w)
}

/// Right-align a `u128` into a 32-byte word.
#[must_use]
pub fn uint_word(value: u128) -> Word {
    let mut w = [0u8; 32];
    w[16..].copy_from_slice(&value.to_be_bytes());
    w
}

/// Build calldata: 4-byte selector followed by the words, concatenated.
#[must_use]
pub fn calldata(selector: [u8; 4], words: &[Word]) -> String {
    let mut out = Vec::with_capacity(4 + words.len() * 32);
    out.extend_from_slice(&selector);
    for w in words {
        out.extend_from_slice(w);
    }
    to_hex(&out)
}

/// Decode a single `uint256` return value into `u128`. Errors if the value does
/// not fit in 128 bits (no token amount sherwood handles comes close).
pub fn decode_u128(ret: &[u8]) -> Result<u128> {
    if ret.len() < 32 {
        return Err(ChainError::Decode(format!(
            "expected a 32-byte word, got {} bytes",
            ret.len()
        )));
    }
    let word = &ret[..32];
    if word[..16].iter().any(|&b| b != 0) {
        return Err(ChainError::Decode(
            "uint256 return exceeds u128 — value too large for this reader".into(),
        ));
    }
    let mut buf = [0u8; 16];
    buf.copy_from_slice(&word[16..32]);
    Ok(u128::from_be_bytes(buf))
}

/// Decode a single `address` return value (the low 20 bytes of the first word),
/// returned `0x`-prefixed lowercase.
pub fn decode_address(ret: &[u8]) -> Result<String> {
    if ret.len() < 32 {
        return Err(ChainError::Decode(
            "expected a 32-byte word for address".into(),
        ));
    }
    Ok(to_hex(&ret[12..32]))
}

/// Decode a dynamic `string` return value: `(offset)(length)(bytes)`. Falls back
/// to a trimmed UTF-8 read of the whole payload for the non-standard `bytes32`
/// name/symbol some old tokens use.
pub fn decode_string(ret: &[u8]) -> Result<String> {
    if ret.len() >= 96 {
        let len = decode_u128(&ret[32..64])? as usize;
        let start = 64;
        if len > 0 && start + len <= ret.len() {
            return Ok(String::from_utf8_lossy(&ret[start..start + len]).into_owned());
        }
    }
    let trimmed: Vec<u8> = ret.iter().copied().take_while(|&b| b != 0).collect();
    Ok(String::from_utf8_lossy(&trimmed).trim().to_owned())
}

/// The 4-byte selector at the front of a revert payload, `0x`-prefixed, if the
/// payload is at least that long.
#[must_use]
pub fn revert_selector(data: Option<&str>) -> Option<String> {
    let raw = from_hex(data?).ok()?;
    (raw.len() >= 4).then(|| to_hex(&raw[..4]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_word_left_pads() {
        let w = address_word("0x0bd7d308f8e1639fab988df18a8011f41eacad73").unwrap();
        assert_eq!(&w[..12], &[0u8; 12]);
        assert_eq!(
            to_hex(&w[12..]),
            "0x0bd7d308f8e1639fab988df18a8011f41eacad73"
        );
    }

    #[test]
    fn address_word_rejects_wrong_length() {
        assert!(address_word("0x1234").is_err());
    }

    #[test]
    fn calldata_concatenates_selector_and_words() {
        let cd = calldata(
            [0x70, 0xa0, 0x82, 0x31],
            &[address_word("0x0000000000000000000000000000000000000001").unwrap()],
        );
        assert_eq!(
            cd,
            "0x70a082310000000000000000000000000000000000000000000000000000000000000001"
        );
    }

    #[test]
    fn decode_u128_reads_a_right_aligned_value() {
        let mut w = [0u8; 32];
        w[16..].copy_from_slice(&1_000_000_000_000_000_000u128.to_be_bytes());
        assert_eq!(decode_u128(&w).unwrap(), 1_000_000_000_000_000_000);
    }

    #[test]
    fn decode_u128_rejects_values_over_128_bits() {
        let mut w = [0u8; 32];
        w[0] = 1;
        assert!(decode_u128(&w).is_err());
    }

    #[test]
    fn decode_string_reads_offset_length_bytes() {
        // abi.encode("NVDA")
        let hexstr = "0000000000000000000000000000000000000000000000000000000000000020\
                      0000000000000000000000000000000000000000000000000000000000000004\
                      4e56444100000000000000000000000000000000000000000000000000000000";
        let bytes = hex::decode(hexstr).unwrap();
        assert_eq!(decode_string(&bytes).unwrap(), "NVDA");
    }

    #[test]
    fn revert_selector_extracts_the_first_four_bytes() {
        assert_eq!(
            revert_selector(Some("0xe450d38c0000000000000000000000000000000000000000")).as_deref(),
            Some("0xe450d38c")
        );
        assert_eq!(revert_selector(Some("0x")), None);
        assert_eq!(revert_selector(None), None);
    }
}
