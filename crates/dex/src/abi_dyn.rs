//! Just enough dynamic-type ABI encoding to build a UniversalRouter
//! `execute()` call: `bytes`, `bytes[]`, and a single dynamic-containing
//! struct argument (head/tail with offset words). Static words (`address`,
//! `uint*`, `int*`, `bool`) are [`sherwood_chain::abi`]'s existing helpers.
//!
//! Every byte layout here was cross-checked against `v4-periphery`'s own
//! `CalldataDecoder` — its minimum-length assembly checks (`0x160` for
//! `ExactInputSingleParams`, `0x40` for a `(currency, uint256)` pair) match
//! this encoder's output length exactly for an empty `hookData`, which is
//! strong evidence the layout is right, not just plausible.

/// A 32-byte ABI word.
pub type Word = [u8; 32];

const WORD: usize = 32;

fn pad_to_word(mut data: Vec<u8>) -> Vec<u8> {
    let rem = data.len() % WORD;
    if rem != 0 {
        data.resize(data.len() + (WORD - rem), 0);
    }
    data
}

/// ABI-encode a `bytes` value: `length (word) || data, right-padded`.
#[must_use]
pub fn encode_bytes(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(WORD + data.len());
    out.extend_from_slice(&word_uint(data.len() as u128));
    out.extend_from_slice(data);
    pad_to_word(out)
}

/// ABI-encode a `bytes[]` value from each element's *raw content* (not yet
/// `encode_bytes`-wrapped — this wraps each one): `length || offset_0..N-1 ||
/// encode_bytes(items[0]) || … `.
#[must_use]
pub fn encode_bytes_array(items: &[Vec<u8>]) -> Vec<u8> {
    let encoded: Vec<Vec<u8>> = items.iter().map(|i| encode_bytes(i)).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&word_uint(items.len() as u128));
    let head_len = items.len() * WORD;
    let mut offset = head_len;
    for e in &encoded {
        out.extend_from_slice(&word_uint(offset as u128));
        offset += e.len();
    }
    for e in encoded {
        out.extend_from_slice(&e);
    }
    out
}

/// ABI-encode two top-level dynamic arguments `(bytes, bytes[])`, i.e. what
/// `abi.encode(actions, params)` (the `V4_SWAP` command's single `input`)
/// produces.
#[must_use]
pub fn encode_bytes_and_bytes_array(a: &[u8], b: &[Vec<u8>]) -> Vec<u8> {
    let enc_a = encode_bytes(a);
    let enc_b = encode_bytes_array(b);
    let mut out = Vec::new();
    out.extend_from_slice(&word_uint(2 * WORD as u128)); // offset to a's data
    out.extend_from_slice(&word_uint((2 * WORD + enc_a.len()) as u128)); // offset to b's data
    out.extend_from_slice(&enc_a);
    out.extend_from_slice(&enc_b);
    out
}

/// ABI-encode a single dynamic-containing struct argument from its
/// already-built head/tail body: `offset(0x20) || body`. This is what
/// `abi.encode(SomeStruct)` produces for a lone struct parameter.
#[must_use]
pub fn encode_single_dynamic_struct(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(WORD + body.len());
    out.extend_from_slice(&word_uint(WORD as u128));
    out.extend_from_slice(body);
    out
}

/// A right-aligned `uint256`/`uint128`/… word.
#[must_use]
pub fn word_uint(v: u128) -> Word {
    let mut w = [0u8; 32];
    w[16..].copy_from_slice(&v.to_be_bytes());
    w
}

/// A `bool` word (`0` or `1`, right-aligned).
#[must_use]
pub fn word_bool(b: bool) -> Word {
    let mut w = [0u8; 32];
    w[31] = u8::from(b);
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_bytes_pads_to_a_word_multiple() {
        let e = encode_bytes(&[0x10]);
        assert_eq!(e.len(), 64); // length word + one padded-to-32 data word
        assert_eq!(e[31], 1); // length = 1
        assert_eq!(e[32], 0x10);
    }

    #[test]
    fn encode_bytes_array_of_one_matches_expected_shape() {
        let e = encode_bytes_array(&[vec![0xaa, 0xbb]]);
        // length(32) + offset(32) + [len(32) + data(32 padded)]
        assert_eq!(e.len(), 32 + 32 + 32 + 32);
        assert_eq!(e[31], 1); // array length = 1
        assert_eq!(u128::from_be_bytes(e[48..64].try_into().unwrap()), 32); // offset = 0x20
    }

    #[test]
    fn word_uint_is_right_aligned() {
        let w = word_uint(0x10);
        assert_eq!(w[31], 0x10);
        assert!(w[..31].iter().all(|&b| b == 0));
    }
}
