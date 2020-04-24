// Copyright 2018-2020 Kodebox, Inc.
// This file is part of CodeChain.
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use super::seal::Seal;
use ccrypto::BLAKE_NULL_RLP;
use ckey::{Ed25519Public as Public, PlatformAddress};
use ctypes::BlockHash;
use primitives::{Bytes, H256};

/// Genesis components.
pub struct Genesis {
    /// Seal.
    pub seal: Seal,
    /// Author.
    pub author: Public,
    /// Timestamp.
    pub timestamp: u64,
    /// Parent hash.
    pub parent_hash: BlockHash,
    /// Transactions root.
    pub transactions_root: H256,
    /// State root.
    pub state_root: Option<H256>,
    /// Next validator set hash.
    pub next_validator_set_hash: Option<H256>,
    /// The genesis block's extra data field.
    pub extra_data: Bytes,
}

impl From<cjson::scheme::Genesis> for Genesis {
    fn from(g: cjson::scheme::Genesis) -> Self {
        Genesis {
            seal: From::from(g.seal),
            author: g.author.map_or_else(Public::default, PlatformAddress::into_pubkey),
            timestamp: g.timestamp.map_or(0, Into::into),
            parent_hash: g.parent_hash.map_or_else(H256::zero, Into::into).into(),
            transactions_root: g.transactions_root.map_or_else(|| BLAKE_NULL_RLP, Into::into),
            state_root: g.state_root.map(Into::into),
            next_validator_set_hash: g.next_validator_set_hash.map(Into::into),
            extra_data: g.extra_data.map_or_else(Vec::new, Into::into),
        }
    }
}
