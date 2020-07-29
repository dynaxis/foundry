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

use crate::views::{BlockView, HeaderView};
use ctypes::BlockHash;
use primitives::Bytes;

/// Describes how best block is changed
#[derive(Debug, Clone, PartialEq)]
pub enum BestBlockChanged {
    /// Cannonical chain is appended.
    CanonChainAppended {
        best_block: Bytes,
    },
    /// Nothing changed.
    None,
}

impl BestBlockChanged {
    pub fn new_best_hash(&self) -> Option<BlockHash> {
        Some(self.best_block()?.hash())
    }

    pub fn best_block(&self) -> Option<BlockView<'_>> {
        let block = match self {
            BestBlockChanged::CanonChainAppended {
                best_block,
            } => best_block,
            BestBlockChanged::None => return None,
        };

        Some(BlockView::new(block))
    }
}

/// Describes how best block is changed
#[derive(Debug, Clone, PartialEq)]
pub enum BestHeaderChanged {
    /// Cannonical chain is appended.
    CanonChainAppended {
        best_header: Vec<u8>,
    },
    /// Nothing changed.
    None,
}

impl BestHeaderChanged {
    pub fn new_best_hash(&self) -> Option<BlockHash> {
        Some(self.header()?.hash())
    }

    pub fn header(&self) -> Option<HeaderView<'_>> {
        let header = match self {
            BestHeaderChanged::CanonChainAppended {
                best_header,
            } => best_header,
            BestHeaderChanged::None => return None,
        };

        Some(HeaderView::new(header))
    }
}
