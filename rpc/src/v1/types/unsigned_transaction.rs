// Copyright 2018-2019 Kodebox, Inc.
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

use super::super::errors;
use cjson::uint::Uint;
use ckey::NetworkId;
use jsonrpc_core::Error;
use std::convert::{TryFrom, TryInto};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsignedTransaction {
    pub network_id: NetworkId,
}
