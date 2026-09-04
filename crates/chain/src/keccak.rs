//! `keccak256` and the two things sherwood derives from it: 4-byte function
//! selectors and 32-byte event topics.
//!
//! Ethereum uses the original Keccak padding, **not** finalised NIST SHA-3 —
//! `sha3::Keccak256`, not `sha3::Sha3_256`.

use sha3::{Digest, Keccak256};

/// `keccak256(bytes)`.
#[must_use]
pub fn keccak256(bytes: &[u8]) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(bytes);
    h.finalize().into()
}

/// The 4-byte function selector for a Solidity signature, e.g.
/// `selector("transfer(address,uint256)") == [0xa9, 0x05, 0x9c, 0xbb]`.
#[must_use]
pub fn selector(signature: &str) -> [u8; 4] {
    let h = keccak256(signature.as_bytes());
    [h[0], h[1], h[2], h[3]]
}

/// The 32-byte event topic (topic0) for an event signature, e.g.
/// `topic("Transfer(address,address,uint256)")`.
#[must_use]
pub fn topic(signature: &str) -> [u8; 32] {
    keccak256(signature.as_bytes())
}

/// `0x`-prefixed lowercase hex of a function selector.
#[must_use]
pub fn selector_hex(signature: &str) -> String {
    format!("0x{}", hex::encode(selector(signature)))
}

/// `0x`-prefixed lowercase hex of an event topic.
#[must_use]
pub fn topic_hex(signature: &str) -> String {
    format!("0x{}", hex::encode(topic(signature)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keccak_of_empty_is_the_known_constant() {
        // The canonical keccak256("") — distinct from SHA3-256("").
        assert_eq!(
            hex::encode(keccak256(b"")),
            "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"
        );
    }

    #[test]
    fn erc20_selectors_match_published_values() {
        assert_eq!(selector_hex("transfer(address,uint256)"), "0xa9059cbb");
        assert_eq!(
            selector_hex("transferFrom(address,address,uint256)"),
            "0x23b872dd"
        );
        assert_eq!(selector_hex("balanceOf(address)"), "0x70a08231");
        assert_eq!(selector_hex("totalSupply()"), "0x18160ddd");
        assert_eq!(selector_hex("decimals()"), "0x313ce567");
        assert_eq!(selector_hex("name()"), "0x06fdde03");
        assert_eq!(selector_hex("symbol()"), "0x95d89b41");
        assert_eq!(selector_hex("approve(address,uint256)"), "0x095ea7b3");
        assert_eq!(selector_hex("paused()"), "0x5c975abb");
        assert_eq!(selector_hex("implementation()"), "0x5c60da1b");
    }

    #[test]
    fn transfer_event_topic_matches_the_published_value() {
        assert_eq!(
            topic_hex("Transfer(address,address,uint256)"),
            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
        );
    }

    #[test]
    fn oz_custom_error_selectors_match() {
        // Used by `probe` to tell "no balance" (a normal ERC-20) apart from a
        // compliance revert.
        assert_eq!(
            selector_hex("ERC20InsufficientBalance(address,uint256,uint256)"),
            "0xe450d38c"
        );
        assert_eq!(
            selector_hex("ERC20InsufficientAllowance(address,uint256,uint256)"),
            "0xfb8f41b2"
        );
    }
}
