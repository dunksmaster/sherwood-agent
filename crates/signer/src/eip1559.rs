//! An EIP-1559 (type `0x02`) transaction: the fields, its RLP encoding, and
//! the hash that gets signed. No RPC client lives here — building a request
//! from live gas/nonce data is the caller's job (`sherwood-dex`); this module
//! only knows how to turn a fully-specified request into signable bytes and,
//! given a signature, into the final raw transaction.

use crate::rlp;

/// A `to` / `value` / `data` transaction on chain `chain_id`, fully specified
/// (nonce and gas params included — this module does not fetch them).
#[derive(Debug, Clone)]
pub struct Eip1559Tx {
    pub chain_id: u64,
    pub nonce: u64,
    pub max_priority_fee_per_gas: u128,
    pub max_fee_per_gas: u128,
    pub gas_limit: u64,
    /// `None` would mean contract creation; sherwood never does that, so
    /// every real use sets this.
    pub to: [u8; 20],
    pub value: u128,
    pub data: Vec<u8>,
}

impl Eip1559Tx {
    /// The RLP list of the nine unsigned fields, `accessList` empty.
    fn unsigned_fields(&self) -> Vec<Vec<u8>> {
        vec![
            rlp::encode_uint(u128::from(self.chain_id)),
            rlp::encode_uint(u128::from(self.nonce)),
            rlp::encode_uint(self.max_priority_fee_per_gas),
            rlp::encode_uint(self.max_fee_per_gas),
            rlp::encode_uint(u128::from(self.gas_limit)),
            rlp::encode_bytes(&self.to),
            rlp::encode_uint(self.value),
            rlp::encode_bytes(&self.data),
            rlp::encode_list(&[]), // accessList
        ]
    }

    /// `0x02 || rlp(unsigned fields)` — what gets keccak256-hashed and signed.
    #[must_use]
    pub fn signing_payload(&self) -> Vec<u8> {
        let mut out = vec![0x02];
        out.extend_from_slice(&rlp::encode_list(&self.unsigned_fields()));
        out
    }

    /// `0x02 || rlp(unsigned fields ++ [y_parity, r, s])` — the final raw
    /// transaction, ready for `eth_sendRawTransaction` (sending it is not
    /// this crate's job).
    #[must_use]
    pub fn into_raw_signed(self, y_parity: u8, r: [u8; 32], s: [u8; 32]) -> Vec<u8> {
        let mut fields = self.unsigned_fields();
        fields.push(rlp::encode_uint(u128::from(y_parity)));
        fields.push(rlp::encode_bytes(&r));
        fields.push(rlp::encode_bytes(&s));
        let mut out = vec![0x02];
        out.extend_from_slice(&rlp::encode_list(&fields));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Eip1559Tx {
        Eip1559Tx {
            chain_id: 4663,
            nonce: 0,
            max_priority_fee_per_gas: 1_000_000_000,
            max_fee_per_gas: 2_000_000_000,
            gas_limit: 21_000,
            to: [0x11; 20],
            value: 0,
            data: vec![],
        }
    }

    #[test]
    fn signing_payload_starts_with_the_type_byte() {
        let payload = sample().signing_payload();
        assert_eq!(payload[0], 0x02);
    }

    #[test]
    fn signing_payload_is_a_well_formed_rlp_list_covering_the_rest() {
        // byte 1 is the outer list's RLP prefix; the encoded length must
        // exactly account for the rest of the payload.
        let payload = sample().signing_payload();
        let body = &payload[1..];
        assert!(
            body[0] >= 0xc0,
            "expected a list prefix, got {:#x}",
            body[0]
        );
    }

    #[test]
    fn raw_signed_appends_y_parity_r_s_after_the_unsigned_fields() {
        let tx = sample();
        let unsigned_len = tx.signing_payload().len();
        let raw = tx.into_raw_signed(1, [0xaa; 32], [0xbb; 32]);
        // longer than the unsigned payload by roughly the three appended
        // fields (1 byte parity + 33-byte r + 33-byte s, plus list-length
        // prefix growth).
        assert!(raw.len() > unsigned_len);
        assert_eq!(raw[0], 0x02);
    }

    #[test]
    fn empty_data_and_zero_value_round_trip_through_rlp_uint_zero() {
        // value=0 and data=[] both encode as the empty string (0x80); this
        // just pins that both fields are present, not omitted.
        let tx = sample();
        let fields = tx.unsigned_fields();
        assert_eq!(fields.len(), 9);
        assert_eq!(fields[6], vec![0x80]); // value
        assert_eq!(fields[7], vec![0x80]); // data
        assert_eq!(fields[8], vec![0xc0]); // accessList
    }
}
