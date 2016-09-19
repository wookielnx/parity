// Copyright 2015, 2016 Ethcore (UK) Ltd.
// This file is part of Parity.

// Parity is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Parity.  If not, see <http://www.gnu.org/licenses/>.

#![warn(missing_docs)]
#![cfg_attr(feature="dev", feature(plugin))]
#![cfg_attr(feature="dev", plugin(clippy))]
// Keeps consistency (all lines with `.clone()`) and helpful when changing ref to non-ref.
#![cfg_attr(feature="dev", allow(clone_on_copy))]
// In most cases it expresses function flow better
#![cfg_attr(feature="dev", allow(if_not_else))]

//! Blockchain sync module
//! Implements ethereum protocol version 63 as specified here:
//! https://github.com/ethereum/wiki/wiki/Ethereum-Wire-Protocol
//!

extern crate ethcore_network as network;
extern crate ethcore_io as io;
extern crate ethcore;
extern crate env_logger;
extern crate time;
extern crate rand;
extern crate semver;
extern crate parking_lot;
extern crate rlp;

#[macro_use]
extern crate log;
#[macro_use]
extern crate ethcore_util as util;
#[macro_use]
extern crate heapsize;
#[macro_use]
extern crate ethcore_ipc as ipc;

mod chain;
mod blocks;
mod block_sync;
mod sync_io;
mod snapshot;

#[cfg(test)]
mod tests;

mod api {
	#![allow(dead_code, unused_assignments, unused_variables, missing_docs)] // codegen issues
	include!(concat!(env!("OUT_DIR"), "/api.rs"));
}

pub use api::{EthSync, SyncProvider, SyncClient, NetworkManagerClient, ManageNetwork, SyncConfig,
	ServiceConfiguration, NetworkConfiguration};
pub use chain::{SyncStatus, SyncState};
pub use network::{is_valid_node_url, NonReservedPeerMode, NetworkError};
