// Copyright 2018-2020 Kodebox, Inc.
// This file is part of CodeChain.
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use crate::{BlockHash, BlockNumber};
use ccrypto::{blake256, BLAKE_NULL_RLP};
use ckey::Ed25519Public as Public;
use primitives::{Bytes, H256, U256};
use rlp::*;
use std::cell::RefCell;
use std::cmp;
use std::time::{SystemTime, UNIX_EPOCH};

/// Semantic boolean for when a seal/signature is included.
pub enum Seal {
    /// The seal/signature is included.
    With,
    /// The seal/signature is not included.
    Without,
}

/// A block header.
/// Note : you must modify /core/src/views/header.rs too when you modify this.
#[derive(Debug, Clone)]
pub struct Header {
    /// Parent hash.
    parent_hash: BlockHash,
    /// Block timestamp.
    timestamp: u64,
    /// Block number.
    number: BlockNumber,
    /// Block author.
    author: Public,

    /// Block extra data.
    extra_data: Bytes,

    /// Evidences root
    evidenecs_root: H256,
    /// Transactions root.
    transactions_root: H256,
    /// State root.
    state_root: H256,
    /// Next validator set hash.
    next_validator_set_hash: H256,

    /// Vector of post-RLP-encoded fields.
    seal: Vec<Bytes>,

    /// The memoized hash of the RLP representation *including* the seal fields.
    hash: RefCell<Option<H256>>,
    /// The memoized hash of the RLP representation *without* the seal fields.
    bare_hash: RefCell<Option<H256>>,
}

impl Default for Header {
    /// Create a new, default-valued, header.
    fn default() -> Self {
        Header {
            parent_hash: H256::default().into(),
            timestamp: 0,
            number: 0,
            author: Default::default(),
            extra_data: vec![],

            evidenecs_root: BLAKE_NULL_RLP,
            transactions_root: BLAKE_NULL_RLP,
            state_root: BLAKE_NULL_RLP,
            next_validator_set_hash: BLAKE_NULL_RLP,

            seal: vec![],
            hash: RefCell::new(None),
            bare_hash: RefCell::new(None),
        }
    }
}

const SIZE_WITHOUT_SEAL: usize = 9;

impl Header {
    /// Create a new, default-valued, header.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the parent_hash field of the header.
    pub fn parent_hash(&self) -> &BlockHash {
        &self.parent_hash
    }
    /// Get the timestamp field of the header.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }
    /// Get the number field of the header.
    pub fn number(&self) -> BlockNumber {
        self.number
    }
    /// Get the author field of the header.
    pub fn author(&self) -> &Public {
        &self.author
    }

    /// Get the extra data field of the header.
    pub fn extra_data(&self) -> &Bytes {
        &self.extra_data
    }
    /// Get a mutable reference to extra_data
    pub fn extra_data_mut(&mut self) -> &mut Bytes {
        self.note_dirty();
        &mut self.extra_data
    }

    /// Get the state root field of the header.
    pub fn state_root(&self) -> &H256 {
        &self.state_root
    }

    /// Get the evidences root field of the header.
    pub fn evidences_root(&self) -> &H256 {
        &self.evidenecs_root
    }

    /// Get the transactions root field of the header.
    pub fn transactions_root(&self) -> &H256 {
        &self.transactions_root
    }

    /// Get the validator set root field of the header.
    pub fn next_validator_set_hash(&self) -> &H256 {
        &self.next_validator_set_hash
    }

    /// Get whether the block has transactions.
    pub fn is_empty(&self) -> bool {
        self.transactions_root() == &BLAKE_NULL_RLP
    }

    /// Get the seal field of the header.
    pub fn seal(&self) -> &[Bytes] {
        &self.seal
    }

    /// Get view in the seal field of the header.
    pub fn view(&self) -> u64 {
        let seal = self.seal();
        if let Some(rlp_view) = seal.get(1) {
            Rlp::new(rlp_view.as_slice()).as_val().unwrap()
        } else {
            0
        }
    }

    /// Set the number field of the header.
    pub fn set_parent_hash(&mut self, a: BlockHash) {
        self.parent_hash = a;
        self.note_dirty();
    }
    /// Set the timestamp field of the header.
    pub fn set_timestamp(&mut self, a: u64) {
        self.timestamp = a;
        self.note_dirty();
    }
    /// Set the timestamp field of the header to the current time.
    pub fn set_timestamp_now(&mut self, but_later_than: u64) {
        self.timestamp = cmp::max(
            SystemTime::now().duration_since(UNIX_EPOCH).expect("There is no time machine.").as_secs(),
            but_later_than,
        );
        self.note_dirty();
    }
    /// Set the number field of the header.
    pub fn set_number(&mut self, a: BlockNumber) {
        self.number = a;
        self.note_dirty();
    }
    /// Set the author field of the header.
    pub fn set_author(&mut self, a: Public) {
        if a != self.author {
            self.author = a;
            self.note_dirty();
        }
    }
    /// Set the extra data field of the header.
    pub fn set_extra_data(&mut self, a: Bytes) {
        if a != self.extra_data {
            self.extra_data = a;
            self.note_dirty();
        }
    }

    /// Set the state root field of the header.
    pub fn set_state_root(&mut self, a: H256) {
        self.state_root = a;
        self.note_dirty();
    }
    /// Set the evidences root field of the header.
    pub fn set_evidences_root(&mut self, a: H256) {
        self.evidenecs_root = a;
        self.note_dirty();
    }
    /// Set the transactions root field of the header.
    pub fn set_transactions_root(&mut self, a: H256) {
        self.transactions_root = a;
        self.note_dirty()
    }
    /// Set the validator set root field of the header.
    pub fn set_next_validator_set_hash(&mut self, a: H256) {
        self.next_validator_set_hash = a;
        self.note_dirty()
    }
    /// Set the seal field of the header.
    pub fn set_seal(&mut self, a: Vec<Bytes>) {
        self.seal = a;
        self.note_dirty();
    }

    /// Get the hash of this header (blake of the RLP).
    pub fn hash(&self) -> BlockHash {
        let mut hash = self.hash.borrow_mut();
        match &mut *hash {
            Some(h) => (*h).into(),
            hash @ &mut None => {
                let h = self.rlp_blake(&Seal::With);
                *hash = Some(h);
                h.into()
            }
        }
    }

    /// Get the hash of the header excluding the seal
    pub fn bare_hash(&self) -> H256 {
        let mut hash = self.bare_hash.borrow_mut();
        match &mut *hash {
            Some(h) => *h,
            hash @ None => {
                let h = self.rlp_blake(&Seal::Without);
                *hash = Some(h);
                h
            }
        }
    }

    /// Place this header into an RLP stream `s`, optionally `with_seal`.
    pub fn stream_rlp(&self, s: &mut RlpStream, with_seal: &Seal) {
        s.begin_list(
            SIZE_WITHOUT_SEAL
                + match with_seal {
                    Seal::With => self.seal.len(),
                    _ => 0,
                },
        );
        s.append(&self.parent_hash);
        s.append(&self.author);
        s.append(&self.state_root);
        s.append(&self.evidenecs_root);
        s.append(&self.transactions_root);
        s.append(&self.next_validator_set_hash);
        s.append(&self.number);
        s.append(&self.timestamp);
        s.append(&self.extra_data);
        if let Seal::With = with_seal {
            for b in &self.seal {
                s.append_raw(b, 1);
            }
        }
    }

    /// Get the RLP of this header, optionally `with_seal`.
    pub fn rlp(&self, with_seal: &Seal) -> Bytes {
        let mut s = RlpStream::new();
        self.stream_rlp(&mut s, with_seal);
        s.out()
    }

    /// Note that some fields have changed. Resets the memoised hash.
    pub fn note_dirty(&self) {
        *self.hash.borrow_mut() = None;
        *self.bare_hash.borrow_mut() = None;
    }

    /// Get the Blake hash of this header, optionally `with_seal`.
    pub fn rlp_blake(&self, with_seal: &Seal) -> H256 {
        blake256(&self.rlp(with_seal))
    }

    pub fn generate_child(&self) -> Self {
        let mut header = Header::default();

        header.set_parent_hash(self.hash());
        header.set_number(self.number() + 1);
        header.set_timestamp_now(self.timestamp() + 1);
        header.note_dirty();

        header
    }
}

impl Decodable for Header {
    fn decode(r: &Rlp<'_>) -> Result<Self, DecoderError> {
        let mut blockheader = Header {
            parent_hash: r.val_at(0)?,
            author: r.val_at(1)?,
            state_root: r.val_at(2)?,
            evidenecs_root: r.val_at(3)?,
            transactions_root: r.val_at(4)?,
            next_validator_set_hash: r.val_at(5)?,
            number: r.val_at(6)?,
            timestamp: cmp::min(r.val_at::<U256>(7)?, u64::max_value().into()).as_u64(),
            extra_data: r.val_at(8)?,
            seal: vec![],
            hash: RefCell::new(Some(blake256(r.as_raw()))),
            bare_hash: RefCell::new(None),
        };

        for i in SIZE_WITHOUT_SEAL..r.item_count()? {
            blockheader.seal.push(r.at(i)?.as_raw().to_vec())
        }

        Ok(blockheader)
    }
}

impl Encodable for Header {
    fn rlp_append(&self, s: &mut RlpStream) {
        self.stream_rlp(s, &Seal::With);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_deserialize_test() {
        let empty = Header::default();
        let encoded = rlp::encode(&empty);
        let decoded: Header = rlp::decode(&encoded).unwrap();
        assert_eq!(empty.hash(), decoded.hash());
    }
}
