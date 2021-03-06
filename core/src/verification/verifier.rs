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

use super::verification;
use crate::consensus::ConsensusEngine;
use crate::error::Error;
use ctypes::{ConsensusParams, Header};

/// Should be used to verify blocks.
pub struct Verifier;

impl Verifier {
    /// Verify a block relative to its parent and uncles.
    pub fn verify_block_family(
        &self,
        block: &[u8],
        header: &Header,
        parent: &Header,
        engine: &dyn ConsensusEngine,
        consensus_params: &ConsensusParams,
    ) -> Result<(), Error> {
        verification::verify_block_family(block, header, parent, engine, consensus_params)
    }

    /// Do a final verification check for an enacted header vs its expected counterpart.
    pub fn verify_block_final(&self, expected: &Header, got: &Header) -> Result<(), Error> {
        verification::verify_block_final(expected, got)
    }

    /// Verify a block, inspecting external state.
    pub fn verify_block_external(&self, header: &Header, engine: &dyn ConsensusEngine) -> Result<(), Error> {
        engine.verify_block_external(header)
    }
}
