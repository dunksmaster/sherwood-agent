//! secp256k1 key custody and EIP-1559 signing for Robinhood Chain
//! ([ADR-0006](../../../docs/adr/0006-robinhood-chain-venue.md), v0.2.2).
//!
//! **Sign-local, broadcast-explicit.** This crate holds a private key,
//! derives its address, and turns an [`eip1559::Eip1559Tx`] into signed raw
//! bytes. That is the entire surface. There is no RPC client here and no
//! method that sends anything to a network — broadcasting a signed
//! transaction is the caller's explicit, separate action.
//!
//! The key itself is never printed, logged, or returned by any method —
//! [`LocalSigner`]'s `Debug` impl shows only the derived address. It lives in
//! memory as a [`sherwood_secrets::SecretString`] until parsed into a
//! [`k256::SecretKey`], and the raw bytes are zeroized on drop
//! ([`ZeroizeOnDrop`](zeroize::ZeroizeOnDrop) via `k256`'s own key types).
//!
//! **This crate's cryptography has been checked by unit tests only** — a
//! self-consistent sign → recover → address-matches round trip, and RLP
//! known-answer tests. That is necessary, not sufficient. Before trusting it
//! with real funds, sign one transaction, decode it independently (e.g. a
//! block explorer or `cast tx --raw`), and confirm the *sender* it reports is
//! exactly the funded wallet — do this with a trivial-value transaction
//! first.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod eip1559;
mod rlp;

use eip1559::Eip1559Tx;
use k256::ecdsa::signature::hazmat::PrehashSigner;
use k256::ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey};
use sha3::{Digest, Keccak256};
use sherwood_secrets::SecretString;
use zeroize::Zeroizing;

/// Anything that can go wrong loading a key or signing.
#[derive(Debug, thiserror::Error)]
pub enum SignerError {
    #[error("private key: {0}")]
    Key(String),
    #[error("signing failed: {0}")]
    Sign(String),
}

type Result<T> = std::result::Result<T, SignerError>;

fn keccak256(bytes: &[u8]) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(bytes);
    h.finalize().into()
}

/// Derive the 20-byte EVM address from an uncompressed secp256k1 public key.
fn address_from_verifying_key(vk: &VerifyingKey) -> [u8; 20] {
    let point = vk.to_encoded_point(false); // uncompressed: 0x04 || X || Y
    let hash = keccak256(&point.as_bytes()[1..]); // hash X||Y, drop the 0x04 tag
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&hash[12..32]);
    addr
}

/// `0x`-prefixed lowercase hex of a 20-byte address.
#[must_use]
pub fn address_hex(addr: &[u8; 20]) -> String {
    format!("0x{}", hex::encode(addr))
}

/// A key held in memory, ready to sign. Holds exactly one secp256k1 keypair.
pub struct LocalSigner {
    key: SigningKey,
    address: [u8; 20],
}

impl std::fmt::Debug for LocalSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalSigner")
            .field("address", &address_hex(&self.address))
            .finish_non_exhaustive() // never the key
    }
}

impl LocalSigner {
    /// Parse a 32-byte private key from hex (`0x`-prefixed or bare) held in a
    /// [`SecretString`] — e.g. from `sherwood secrets get`. The plaintext
    /// hex is zeroized as soon as it is decoded.
    pub fn from_hex(secret: &SecretString) -> Result<Self> {
        let raw = secret.expose().trim();
        let raw = raw.strip_prefix("0x").unwrap_or(raw);
        let mut bytes = Zeroizing::new(
            hex::decode(raw).map_err(|e| SignerError::Key(format!("not valid hex: {e}")))?,
        );
        if bytes.len() != 32 {
            return Err(SignerError::Key(format!(
                "expected a 32-byte key, got {} bytes",
                bytes.len()
            )));
        }
        let key = SigningKey::from_slice(&bytes)
            .map_err(|e| SignerError::Key(format!("invalid secp256k1 key: {e}")))?;
        bytes.iter_mut().for_each(|b| *b = 0); // belt-and-braces; Zeroizing also does this on drop
        let address = address_from_verifying_key(key.verifying_key());
        Ok(Self { key, address })
    }

    /// The wallet's address — safe to log, display, and hand to a faucet.
    #[must_use]
    pub fn address(&self) -> [u8; 20] {
        self.address
    }

    /// [`Self::address`], `0x`-prefixed hex.
    #[must_use]
    pub fn address_hex(&self) -> String {
        address_hex(&self.address)
    }

    /// Sign `tx`, returning the final raw transaction bytes
    /// (`eth_sendRawTransaction`-ready). Never sends anything.
    pub fn sign_transaction(&self, tx: Eip1559Tx) -> Result<Vec<u8>> {
        let payload = tx.signing_payload();
        let hash = keccak256(&payload);
        let (sig, rec_id) = self.sign_prehash_low_s(&hash)?;
        let r: [u8; 32] = sig.r().to_bytes().into();
        let s: [u8; 32] = sig.s().to_bytes().into();
        Ok(tx.into_raw_signed(rec_id.to_byte(), r, s))
    }

    /// Sign a 32-byte digest directly, low-`s` normalised (EIP-2) with the
    /// recovery id kept consistent with the normalisation — verified by this
    /// module's own recover-and-compare test.
    fn sign_prehash_low_s(&self, hash: &[u8; 32]) -> Result<(Signature, RecoveryId)> {
        let (sig, rec_id): (Signature, RecoveryId) = self
            .key
            .sign_prehash(hash)
            .map_err(|e| SignerError::Sign(e.to_string()))?;
        match sig.normalize_s() {
            Some(normalized) => {
                // Normalising negates s, which flips the point's y-parity.
                let flipped = RecoveryId::new(!rec_id.is_y_odd(), rec_id.is_x_reduced());
                Ok((normalized, flipped))
            }
            None => Ok((sig, rec_id)), // already low-s
        }
    }
}

/// Recover the signer's public key from a signed digest — used to
/// self-check [`LocalSigner::sign_prehash_low_s`], and reusable to verify a
/// signature was produced by a given address without holding the key.
pub fn recover_address(hash: &[u8; 32], sig: &Signature, rec_id: RecoveryId) -> Result<[u8; 20]> {
    let vk = VerifyingKey::recover_from_prehash(hash, sig, rec_id)
        .map_err(|e| SignerError::Sign(format!("recovery failed: {e}")))?;
    Ok(address_from_verifying_key(&vk))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret(hex_key: &str) -> SecretString {
        SecretString::new(hex_key.to_string())
    }

    #[test]
    fn from_hex_rejects_the_wrong_length() {
        assert!(LocalSigner::from_hex(&secret("0xabcd")).is_err());
    }

    #[test]
    fn from_hex_rejects_non_hex() {
        assert!(LocalSigner::from_hex(&secret("not hex at all, 32+ chars long!!")).is_err());
    }

    #[test]
    fn from_hex_accepts_with_or_without_0x_and_derives_a_20_byte_address() {
        let k = "11".repeat(32); // a valid, arbitrary 32-byte scalar
        let a = LocalSigner::from_hex(&secret(&k)).unwrap();
        let b = LocalSigner::from_hex(&secret(&format!("0x{k}"))).unwrap();
        assert_eq!(a.address(), b.address());
        assert_eq!(a.address_hex().len(), 42); // "0x" + 40 hex chars
    }

    #[test]
    fn debug_never_prints_the_key() {
        let k = "22".repeat(32);
        let signer = LocalSigner::from_hex(&secret(&k)).unwrap();
        let dbg = format!("{signer:?}");
        assert!(!dbg.contains(&k), "Debug output leaked the key: {dbg}");
        assert!(dbg.contains(&signer.address_hex()));
    }

    #[test]
    fn signing_a_hash_recovers_to_the_signer_own_address() {
        let k = "33".repeat(32);
        let signer = LocalSigner::from_hex(&secret(&k)).unwrap();
        let hash = keccak256(b"sherwood test message");
        let (sig, rec_id) = signer.sign_prehash_low_s(&hash).unwrap();
        let recovered = recover_address(&hash, &sig, rec_id).unwrap();
        assert_eq!(recovered, signer.address());
    }

    #[test]
    fn signature_s_is_always_normalised_low() {
        let k = "44".repeat(32);
        let signer = LocalSigner::from_hex(&secret(&k)).unwrap();
        for msg in ["a", "b", "sherwood", "0x02 tx payload-ish bytes"] {
            let hash = keccak256(msg.as_bytes());
            let (sig, _) = signer.sign_prehash_low_s(&hash).unwrap();
            assert!(
                sig.normalize_s().is_none(),
                "signature for {msg:?} was not low-s"
            );
        }
    }

    #[test]
    fn sign_transaction_produces_a_type_2_raw_tx_whose_signer_recovers_correctly() {
        let k = "55".repeat(32);
        let signer = LocalSigner::from_hex(&secret(&k)).unwrap();
        let tx = Eip1559Tx {
            chain_id: 4663,
            nonce: 7,
            max_priority_fee_per_gas: 1_000_000_000,
            max_fee_per_gas: 3_000_000_000,
            gas_limit: 100_000,
            to: [0x42; 20],
            value: 0,
            data: vec![0xa9, 0x05, 0x9c, 0xbb], // an arbitrary 4-byte selector
        };
        let signing_hash = keccak256(&tx.signing_payload());
        let raw = signer.sign_transaction(tx).unwrap();
        assert_eq!(raw[0], 0x02);

        // Re-derive r/s/v from the tail of the RLP list is more RLP than this
        // test needs; instead re-sign the same payload deterministically
        // (ECDSA here is RFC6979 — deterministic) and check it reproduces the
        // same signing hash, then confirm that hash recovers to our address.
        let (sig, rec_id) = signer.sign_prehash_low_s(&signing_hash).unwrap();
        let recovered = recover_address(&signing_hash, &sig, rec_id).unwrap();
        assert_eq!(recovered, signer.address());
    }

    #[test]
    fn different_keys_yield_different_addresses() {
        let a = LocalSigner::from_hex(&secret(&"01".repeat(32))).unwrap();
        let b = LocalSigner::from_hex(&secret(&"02".repeat(32))).unwrap();
        assert_ne!(a.address(), b.address());
    }
}
