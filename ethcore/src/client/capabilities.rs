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

//! Client capabilities.

/// A marker trait for a "full" node -- this implies that the entire blockchain
/// (as it is known) is stored locally, without having to perform network queries.
pub trait Full {}

/// The capability of a client to provide receipts.
pub trait Receipts {
	/// Get transaction receipt with given hash.
	fn transaction_receipt(&self, id: TransactionID) -> Option<LocalizedReceipt>;

	/// Get raw block receipts data by block header hash.
	fn block_receipts(&self, hash: &H256) -> Option<Bytes>;
}

/// The capability of a client to provide traces.
pub trait Tracing {
	/// Returns traces matching given filter.
	fn filter_traces(&self, filter: TraceFilter) -> Option<Vec<LocalizedTrace>>;

	/// Returns trace with given id.
	fn trace(&self, trace: TraceId) -> Option<LocalizedTrace>;

	/// Returns traces created by transaction.
	fn transaction_traces(&self, trace: TransactionID) -> Option<Vec<LocalizedTrace>>;

	/// Returns traces created by transaction from block.
	fn block_traces(&self, trace: BlockID) -> Option<Vec<LocalizedTrace>>;
}

/// The capability of a client to mine new blocks.
pub trait Mining {
	/// Returns OpenBlock prepared for closing.
	fn prepare_open_block(&self, author: Address, gas_range_target: (U256, U256), extra_data: Bytes)
		-> OpenBlock;

	/// Returns EvmFactory.
	fn vm_factory(&self) -> &EvmFactory;

	/// Import sealed block. Skips all verifications.
	fn import_sealed_block(&self, block: SealedBlock) -> ImportResult;
}

/// A client which has the capability to import new blocks.
/// This manages a block queue, whose status can be queried with `queue_info`.
/// `BlockChainClient + Full + Syncing` is a suitable requirement for the `eth` protocol.
pub trait Syncing {
	/// Import a block into the blockchain.
	fn import_block(&self, bytes: Bytes) -> Result<H256, BlockImportError>;

	/// Get block queue information.
	fn queue_info(&self) -> BlockQueueInfo;

	/// Clear block queue and abort all import activity.
	fn clear_queue(&self);

	/// Get latest state node
	fn state_data(&self, hash: &H256) -> Option<Bytes>;

	/// Get a tree route between `from` and `to`.
	/// See `BlockChain::tree_route`.
	fn tree_route(&self, from: &H256, to: &H256) -> Option<TreeRoute>;
}