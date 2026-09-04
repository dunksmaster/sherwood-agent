//! A minimal RLP encoder — just enough to build an EIP-1559 transaction.
//! Not a general RLP library: no decoding, no nested-list helpers beyond what
//! [`crate::eip1559`] needs.
//!
//! RLP (Ethereum's Recursive Length Prefix): a byte string 0–55 bytes long is
//! `0x80+len` followed by the bytes (a single byte `< 0x80` encodes as
//! itself); longer strings are `0xb7+len(len)`, the big-endian length, then
//! the bytes. Lists follow the same shape with `0xc0`/`0xf7` bases over the
//! concatenated encoding of their items.

/// RLP-encode a byte string.
#[must_use]
pub fn encode_bytes(data: &[u8]) -> Vec<u8> {
    if data.len() == 1 && data[0] < 0x80 {
        return vec![data[0]];
    }
    let mut out = length_prefix(0x80, 0xb7, data.len());
    out.extend_from_slice(data);
    out
}

/// RLP-encode a list from the already-encoded bytes of its items.
#[must_use]
pub fn encode_list(items: &[Vec<u8>]) -> Vec<u8> {
    let body: usize = items.iter().map(Vec::len).sum();
    let mut out = length_prefix(0xc0, 0xf7, body);
    for item in items {
        out.extend_from_slice(item);
    }
    out
}

/// RLP-encode a non-negative integer as its minimal big-endian byte string
/// (RLP has no separate integer type — `0` encodes as the empty string).
#[must_use]
pub fn encode_uint(value: u128) -> Vec<u8> {
    if value == 0 {
        return encode_bytes(&[]);
    }
    let be = value.to_be_bytes();
    let first_nonzero = be.iter().position(|&b| b != 0).unwrap_or(be.len() - 1);
    encode_bytes(&be[first_nonzero..])
}

fn length_prefix(short_base: u8, long_base: u8, len: usize) -> Vec<u8> {
    if len <= 55 {
        vec![short_base + u8::try_from(len).unwrap_or(55)]
    } else {
        let len_bytes = minimal_be(len as u128);
        let mut out = Vec::with_capacity(1 + len_bytes.len());
        out.push(long_base + u8::try_from(len_bytes.len()).unwrap_or(8));
        out.extend_from_slice(&len_bytes);
        out
    }
}

fn minimal_be(v: u128) -> Vec<u8> {
    let be = v.to_be_bytes();
    let first_nonzero = be.iter().position(|&b| b != 0).unwrap_or(be.len() - 1);
    be[first_nonzero..].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Known-answer tests against the canonical RLP examples
    // (ethereum.org / the Yellow Paper's worked examples).
    #[test]
    fn empty_string_encodes_to_0x80() {
        assert_eq!(encode_bytes(&[]), vec![0x80]);
    }

    #[test]
    fn a_single_byte_below_0x80_encodes_as_itself() {
        assert_eq!(encode_bytes(&[0x00]), vec![0x00]);
        assert_eq!(encode_bytes(&[0x7f]), vec![0x7f]);
    }

    #[test]
    fn a_single_byte_at_or_above_0x80_gets_a_length_prefix() {
        assert_eq!(encode_bytes(&[0x80]), vec![0x81, 0x80]);
    }

    #[test]
    fn short_string_dog_matches_the_canonical_example() {
        assert_eq!(encode_bytes(b"dog"), vec![0x83, b'd', b'o', b'g']);
    }

    #[test]
    fn long_string_over_55_bytes_uses_the_long_form() {
        let data = vec![b'a'; 56];
        let enc = encode_bytes(&data);
        assert_eq!(enc[0], 0xb8); // 0xb7 + 1 length-of-length byte
        assert_eq!(enc[1], 56);
        assert_eq!(&enc[2..], &data[..]);
    }

    #[test]
    fn empty_list_encodes_to_0xc0() {
        assert_eq!(encode_list(&[]), vec![0xc0]);
    }

    #[test]
    fn cat_dog_list_matches_the_canonical_example() {
        let enc = encode_list(&[encode_bytes(b"cat"), encode_bytes(b"dog")]);
        assert_eq!(
            enc,
            vec![0xc8, 0x83, b'c', b'a', b't', 0x83, b'd', b'o', b'g']
        );
    }

    #[test]
    fn uint_zero_is_the_empty_string() {
        assert_eq!(encode_uint(0), vec![0x80]);
    }

    #[test]
    fn uint_strips_leading_zero_bytes() {
        assert_eq!(encode_uint(1), vec![0x01]);
        assert_eq!(encode_uint(1024), vec![0x82, 0x04, 0x00]);
    }
}
