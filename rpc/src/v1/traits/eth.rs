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

//! Eth rpc interface.
use std::sync::Arc;
use jsonrpc_core::{Error, from_params, IoDelegate, Params, Ready, to_value, Value};

use ethcore::transaction::SignedTransaction;
use util::{Address, U256, H256, H64};
use rlp::{UntrustedRlp, View};

use v1::types::{H160 as RpcH160, H256 as RpcH256, H64 as RpcH64, U256 as RpcU256};
use v1::types::{Block, BlockNumber, Bytes, CallRequest, Filter, FilterChanges, Index, Log, Receipt, SyncStatus, Transaction};
use v1::helpers::params::{expect_no_params, from_params_default_second, from_params_default_third};

/// Eth rpc implementation.
pub trait Eth: Sized + Send + Sync + 'static {
	/// Called whenever a request is received.
	/// By default, does nothing.
	fn active(&self) -> Result<(), Error> { Ok(()) }

	/// Returns `eth` protocol version.
	fn protocol_version(&self) -> Result<u32, Error>;

	/// Return the sync status.
	fn syncing(&self) -> Result<SyncStatus, Error>;

	/// Returns the number of hashes per second that the node is mining with.
	fn hashrate(&self) -> Result<U256, Error>;

	/// Get the current author for mining.
	fn author(&self) -> Result<Address, Error>;

	/// Whether the node is currently mining.
	fn is_mining(&self) -> Result<bool, Error>;

	/// Returns the current gas price.
	fn gas_price(&self) -> Result<U256, Error>;

	/// Returns a list of accounts.
	fn accounts(&self) -> Result<Vec<Address>, Error>;

	/// Returns the best block number.
	fn block_number(&self) -> Result<u64, Error>;

	/// Returns the balance of the given account.
	fn balance(&self, address: &Address, at: BlockNumber) -> Result<U256, Error>;

	/// Returns the content of storage at the given address.
	fn storage_at(&self, address: &Address, key: &H256, at: BlockNumber) -> Result<H256, Error>;

	/// Returns a block by its hash.
	fn block_by_hash(&self, hash: &H256, include_txs: bool) -> Result<Option<Block>, Error>;

	/// Returns a block by its number.
	fn block_by_number(&self, num: BlockNumber, include_txs: bool) -> Result<Option<Block>, Error>;

	/// Returns the number of transactions sent from the given address at the given block number.
	fn transaction_count(&self, address: &Address, at: BlockNumber) -> Result<U256, Error>;

	/// Returns the number of transactions in the block with given hash.
	fn block_transaction_count_by_hash(&self, hash: &H256) -> Result<Option<usize>, Error>;

	/// Returns the number of transactions in the block with given number.
	fn block_transaction_count_by_number(&self, num: BlockNumber) -> Result<Option<usize>, Error>;

	/// Returns the number of uncles in the block with given hash.
	fn block_uncles_count_by_hash(&self, hash: &H256) -> Result<Option<usize>, Error>;

	/// Returns the number of uncles in the block with given number.
	fn block_uncles_count_by_number(&self, num: BlockNumber) -> Result<Option<usize>, Error>;

	/// Get the code at the given address as of the block with the given hash.
	fn code_at(&self, address: &Address, at: BlockNumber) -> Result<Vec<u8>, Error>;

	/// Send a raw, signed transaction.
	fn send_raw_transaction(&self, transaction: SignedTransaction) -> Result<H256, Error>;

	/// Call a contract.
	fn call(&self, request: CallRequest, at: BlockNumber) -> Result<Vec<u8>, Error>;

	/// Estimate gas needed for execution of given contract.
	fn estimate_gas(&self, request: CallRequest, at: BlockNumber) -> Result<U256, Error>;

	/// Get transaction by its hash.
	fn transaction_by_hash(&self, hash: &H256) -> Result<Option<Transaction>, Error>;

	/// Returns transaction at given block hash and index.
	fn transaction_by_block_hash_and_index(&self, hash: &H256, index: usize) -> Result<Option<Transaction>, Error>;

	/// Returns transaction by given block number and index.
	fn transaction_by_block_number_and_index(&self, num: BlockNumber, index: usize) -> Result<Option<Transaction>, Error>;

	/// Returns transaction receipt.
	fn transaction_receipt(&self, hash: &H256) -> Result<Option<Receipt>, Error>;

	/// Returns an uncles at given block and index.
	fn uncle_by_block_hash_and_index(&self, hash: &H256, index: usize) -> Result<Option<Block>, Error>;

	/// Returns an uncles at given block and index.
	fn uncle_by_block_number_and_index(&self, num: BlockNumber, index: usize) -> Result<Option<Block>, Error>;

	/// Get a list of supported compilers.
	fn compilers(&self) -> Result<Vec<String>, Error>;

	/// Compiles lll code.
	fn compile_lll(&self, code: String) -> Result<Vec<u8>, Error>;

	/// Compiles solidity.
	fn compile_solidity(&self, code: String) -> Result<Vec<u8>, Error>;

	/// Compiles serpent.
	fn compile_serpent(&self, code: String) -> Result<Vec<u8>, Error>;

	/// Returns logs matching given filter object.
	fn logs(&self, filter: Filter) -> Result<Vec<Log>, Error>;

	/// Returns the hash of the current block, the seedHash, and the boundary condition to be met.
	/// Takes an optional work timeout and returns an optional block number.
	fn work(&self, no_new_work_timeout: Option<u64>)
		-> Result<(H256, H256, H256, Option<u64>), Error>;

	/// Used for submitting a proof-of-work solution.
	/// A return value of `true` signals a good solution, `false` otherwise.
	fn submit_work(&self, nonce: H64, pow_hash: H256, mix_hash: H256) -> Result<bool, Error>;

	/// Submit a mining hashrate. `true` if successful, `false` otherwise.
	fn submit_hashrate(&self, rate: U256, id: H256) -> Result<bool, Error>;
}

/// Eth filters rpc api (polling).
pub trait EthFilter: Sized + Send + Sync + 'static {
	/// Called before each request.
	fn active(&self) -> Result<(), Error> { Ok(()) }

	/// Returns id of new filter.
	fn new_filter(&self, filter: Filter) -> Result<usize, Error>;

	/// Returns id of new block filter.
	fn new_block_filter(&self) -> Result<usize, Error>;

	/// Returns id of new block filter.
	fn new_pending_transaction_filter(&self) -> Result<usize, Error>;

	/// Returns filter changes since last poll.
	fn filter_changes(&self, id: usize) -> Result<FilterChanges, Error>;

	/// Returns all logs matching given filter (in a range 'from' - 'to').
	fn filter_logs(&self, id: usize) -> Result<Vec<Log>, Error>;

	/// Uninstalls filter.
	fn uninstall_filter(&self, id: usize) -> Result<bool, Error>;
}

/// Eth rpc interface.
pub trait EthRpc: Sized + Send + Sync + 'static {
	/// Returns protocol version.
	fn protocol_version(&self, _: Params) -> Result<Value, Error>;

	/// Returns an object with data about the sync status or false. (wtf?)
	fn syncing(&self, _: Params) -> Result<Value, Error>;

	/// Returns the number of hashes per second that the node is mining with.
	fn hashrate(&self, _: Params) -> Result<Value, Error>;

	/// Returns block author.
	fn author(&self, _: Params) -> Result<Value, Error>;

	/// Returns true if client is actively mining new blocks.
	fn is_mining(&self, _: Params) -> Result<Value, Error>;

	/// Returns current gas_price.
	fn gas_price(&self, _: Params) -> Result<Value, Error>;

	/// Returns accounts list.
	fn accounts(&self, _: Params) -> Result<Value, Error>;

	/// Returns highest block number.
	fn block_number(&self, _: Params) -> Result<Value, Error>;

	/// Returns balance of the given account.
	fn balance(&self, _: Params) -> Result<Value, Error>;

	/// Returns content of the storage at given address.
	fn storage_at(&self, _: Params) -> Result<Value, Error>;

	/// Returns block with given hash.
	fn block_by_hash(&self, _: Params) -> Result<Value, Error>;

	/// Returns block with given number.
	fn block_by_number(&self, _: Params) -> Result<Value, Error>;

	/// Returns the number of transactions sent from given address at given time (block number).
	fn transaction_count(&self, _: Params) -> Result<Value, Error>;

	/// Returns the number of transactions in a block with given hash.
	fn block_transaction_count_by_hash(&self, _: Params) -> Result<Value, Error>;

	/// Returns the number of transactions in a block with given block number.
	fn block_transaction_count_by_number(&self, _: Params) -> Result<Value, Error>;

	/// Returns the number of uncles in a block with given hash.
	fn block_uncles_count_by_hash(&self, _: Params) -> Result<Value, Error>;

	/// Returns the number of uncles in a block with given block number.
	fn block_uncles_count_by_number(&self, _: Params) -> Result<Value, Error>;

	/// Returns the code at given address at given time (block number).
	fn code_at(&self, _: Params) -> Result<Value, Error>;

	/// Sends signed transaction.
	fn send_raw_transaction(&self, _: Params) -> Result<Value, Error>;

	/// Call contract.
	fn call(&self, _: Params) -> Result<Value, Error>;

	/// Estimate gas needed for execution of given contract.
	fn estimate_gas(&self, _: Params) -> Result<Value, Error>;

	/// Get transaction by its hash.
	fn transaction_by_hash(&self, _: Params) -> Result<Value, Error>;

	/// Returns transaction at given block hash and index.
	fn transaction_by_block_hash_and_index(&self, _: Params) -> Result<Value, Error>;

	/// Returns transaction by given block number and index.
	fn transaction_by_block_number_and_index(&self, _: Params) -> Result<Value, Error>;

	/// Returns transaction receipt.
	fn transaction_receipt(&self, _: Params) -> Result<Value, Error>;

	/// Returns an uncles at given block and index.
	fn uncle_by_block_hash_and_index(&self, _: Params) -> Result<Value, Error>;

	/// Returns an uncles at given block and index.
	fn uncle_by_block_number_and_index(&self, _: Params) -> Result<Value, Error>;

	/// Returns available compilers.
	fn compilers(&self, _: Params) -> Result<Value, Error>;

	/// Compiles lll code.
	fn compile_lll(&self, _: Params) -> Result<Value, Error>;

	/// Compiles solidity.
	fn compile_solidity(&self, _: Params) -> Result<Value, Error>;

	/// Compiles serpent.
	fn compile_serpent(&self, _: Params) -> Result<Value, Error>;

	/// Returns logs matching given filter object.
	fn logs(&self, _: Params) -> Result<Value, Error>;

	/// Returns the hash of the current block, the seedHash, and the boundary condition to be met.
	fn work(&self, _: Params) -> Result<Value, Error>;

	/// Used for submitting a proof-of-work solution.
	fn submit_work(&self, _: Params) -> Result<Value, Error>;

	/// Used for submitting mining hashrate.
	fn submit_hashrate(&self, _: Params) -> Result<Value, Error>;

	/// Should be used to convert object to io delegate.
	fn to_delegate(self) -> IoDelegate<Self> {
		let mut delegate = IoDelegate::new(Arc::new(self));
		delegate.add_method("eth_protocolVersion", EthRpc::protocol_version);
		delegate.add_method("eth_syncing", EthRpc::syncing);
		delegate.add_method("eth_hashrate", EthRpc::hashrate);
		delegate.add_method("eth_coinbase", EthRpc::author);
		delegate.add_method("eth_mining", EthRpc::is_mining);
		delegate.add_method("eth_gasPrice", EthRpc::gas_price);
		delegate.add_method("eth_accounts", EthRpc::accounts);
		delegate.add_method("eth_blockNumber", EthRpc::block_number);
		delegate.add_method("eth_getBalance", EthRpc::balance);
		delegate.add_method("eth_getStorageAt", EthRpc::storage_at);
		delegate.add_method("eth_getTransactionCount", EthRpc::transaction_count);
		delegate.add_method("eth_getBlockTransactionCountByHash", EthRpc::block_transaction_count_by_hash);
		delegate.add_method("eth_getBlockTransactionCountByNumber", EthRpc::block_transaction_count_by_number);
		delegate.add_method("eth_getUncleCountByBlockHash", EthRpc::block_uncles_count_by_hash);
		delegate.add_method("eth_getUncleCountByBlockNumber", EthRpc::block_uncles_count_by_number);
		delegate.add_method("eth_getCode", EthRpc::code_at);
		delegate.add_method("eth_sendRawTransaction", EthRpc::send_raw_transaction);
		delegate.add_method("eth_call", EthRpc::call);
		delegate.add_method("eth_estimateGas", EthRpc::estimate_gas);
		delegate.add_method("eth_getBlockByHash", EthRpc::block_by_hash);
		delegate.add_method("eth_getBlockByNumber", EthRpc::block_by_number);
		delegate.add_method("eth_getTransactionByHash", EthRpc::transaction_by_hash);
		delegate.add_method("eth_getTransactionByBlockHashAndIndex", EthRpc::transaction_by_block_hash_and_index);
		delegate.add_method("eth_getTransactionByBlockNumberAndIndex", EthRpc::transaction_by_block_number_and_index);
		delegate.add_method("eth_getTransactionReceipt", EthRpc::transaction_receipt);
		delegate.add_method("eth_getUncleByBlockHashAndIndex", EthRpc::uncle_by_block_hash_and_index);
		delegate.add_method("eth_getUncleByBlockNumberAndIndex", EthRpc::uncle_by_block_number_and_index);
		delegate.add_method("eth_getCompilers", EthRpc::compilers);
		delegate.add_method("eth_compileLLL", EthRpc::compile_lll);
		delegate.add_method("eth_compileSolidity", EthRpc::compile_solidity);
		delegate.add_method("eth_compileSerpent", EthRpc::compile_serpent);
		delegate.add_method("eth_getLogs", EthRpc::logs);
		delegate.add_method("eth_getWork", EthRpc::work);
		delegate.add_method("eth_submitWork", EthRpc::submit_work);
		delegate.add_method("eth_submitHashrate", EthRpc::submit_hashrate);
		delegate
	}
}

/// Eth filters rpc api (polling).
// TODO: do filters api properly
pub trait EthFilterRpc: Sized + Send + Sync + 'static {
	/// Returns id of new filter.
	fn new_filter(&self, _: Params) -> Result<Value, Error>;

	/// Returns id of new block filter.
	fn new_block_filter(&self, _: Params) -> Result<Value, Error>;

	/// Returns id of new block filter.
	fn new_pending_transaction_filter(&self, _: Params) -> Result<Value, Error>;

	/// Returns filter changes since last poll.
	fn filter_changes(&self, _: Params) -> Result<Value, Error>;

	/// Returns all logs matching given filter (in a range 'from' - 'to').
	fn filter_logs(&self, _: Params) -> Result<Value, Error>;

	/// Uninstalls filter.
	fn uninstall_filter(&self, _: Params) -> Result<Value, Error>;

	/// Should be used to convert object to io delegate.
	fn to_delegate(self) -> IoDelegate<Self> {
		let mut delegate = IoDelegate::new(Arc::new(self));
		delegate.add_method("eth_newFilter", EthFilterRpc::new_filter);
		delegate.add_method("eth_newBlockFilter", EthFilterRpc::new_block_filter);
		delegate.add_method("eth_newPendingTransactionFilter", EthFilterRpc::new_pending_transaction_filter);
		delegate.add_method("eth_getFilterChanges", EthFilterRpc::filter_changes);
		delegate.add_method("eth_getFilterLogs", EthFilterRpc::filter_logs);
		delegate.add_method("eth_uninstallFilter", EthFilterRpc::uninstall_filter);
		delegate
	}
}

/// Signing methods implementation relying on unlocked accounts.
pub trait EthSigning: Sized + Send + Sync + 'static {
	/// Signs the data with given address signature.
	fn sign(&self, _: Params, _: Ready);

	/// Posts sign request asynchronously.
	/// Will return a confirmation ID for later use with check_transaction.
	fn post_sign(&self, _: Params) -> Result<Value, Error>;

	/// Sends transaction; will block for 20s to try to return the
	/// transaction hash.
	/// If it cannot yet be signed, it will return a transaction ID for
	/// later use with check_transaction.
	fn send_transaction(&self, _: Params, _: Ready);

	/// Posts transaction asynchronously.
	/// Will return a transaction ID for later use with check_transaction.
	fn post_transaction(&self, _: Params) -> Result<Value, Error>;

	/// Checks the progress of a previously posted request (transaction/sign).
	/// Should be given a valid send_transaction ID.
	/// Returns the transaction hash, the zero hash (not yet available),
	/// or the signature,
	/// or an error.
	fn check_request(&self, _: Params) -> Result<Value, Error>;

	/// Should be used to convert object to io delegate.
	fn to_delegate(self) -> IoDelegate<Self> {
		let mut delegate = IoDelegate::new(Arc::new(self));
		delegate.add_async_method("eth_sign", EthSigning::sign);
		delegate.add_async_method("eth_sendTransaction", EthSigning::send_transaction);
		delegate.add_method("eth_postSign", EthSigning::post_sign);
		delegate.add_method("eth_postTransaction", EthSigning::post_transaction);
		delegate.add_method("eth_checkRequest", EthSigning::check_request);
		delegate
	}
}

impl<T: Eth> EthRpc for T {
	fn protocol_version(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		try!(expect_no_params(params));

		let version = try!(Eth::protocol_version(self));

		Ok(Value::String(format!("{}", version)))
	}

	fn syncing(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		try!(expect_no_params(params));

		Eth::syncing(self).map(to_value)
	}

	fn hashrate(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		try!(expect_no_params(params));

		Eth::hashrate(self).map(RpcU256::from).map(to_value)
	}

	fn author(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		try!(expect_no_params(params));

		Eth::author(self).map(RpcH160::from).map(to_value)
	}

	fn is_mining(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		try!(expect_no_params(params));

		Eth::is_mining(self).map(to_value)
	}

	fn gas_price(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		try!(expect_no_params(params));

		Eth::gas_price(self).map(RpcU256::from).map(to_value)
	}

	fn accounts(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		try!(expect_no_params(params));

		Eth::accounts(self)
			.map(|accs| accs.into_iter().map(Into::into).collect::<Vec<RpcH160>>())
			.map(to_value)
	}

	/// Returns highest block number.
	fn block_number(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		try!(expect_no_params(params));

		Eth::block_number(self).map(RpcU256::from).map(to_value)
	}

	/// Returns balance of the given account.
	fn balance(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params_default_second::<RpcH160>(params).and_then(|(address, block_number,)| {
			Eth::balance(self, &address.into(), block_number)
				.map(RpcU256::from).map(to_value)
		})
	}

	/// Returns content of the storage at given address.
	fn storage_at(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params_default_third::<RpcH160, RpcU256>(params)
			.and_then(|(address, position, block_number,)| {
				let position: U256 = position.into();
				Eth::storage_at(self, &address.into(), &position.into(), block_number)
					.map(RpcH256::from).map(to_value)
			})
	}

	/// Returns block with given hash.
	fn block_by_hash(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		from_params::<(RpcH256, bool)>(params).and_then(|(hash, include_txs)| {
			Eth::block_by_hash(self, &hash.into(), include_txs).map(to_value)
		})
	}

	/// Returns block with given number.
	fn block_by_number(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params::<(BlockNumber, bool)>(params).and_then(|(num, include_txs)| {
			Eth::block_by_number(self, num, include_txs).map(to_value)
		})
	}

	/// Returns the number of transactions sent from given address at given time (block number).
	fn transaction_count(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params_default_second::<RpcH160>(params).and_then(|(address, block_number)| {
			Eth::transaction_count(self, &address.into(), block_number)
				.map(RpcU256::from).map(to_value)
		})
	}

	/// Returns the number of transactions in a block with given hash.
	fn block_transaction_count_by_hash(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params::<(RpcH256,)>(params).and_then(|(hash,)| {
			Eth::block_transaction_count_by_hash(self, &hash.into())
				.map(|x| x.map(RpcU256::from))
				.map(to_value)
		})
	}

	/// Returns the number of transactions in a block with given block number.
	fn block_transaction_count_by_number(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params(params).and_then(|(block_number,)| {
			Eth::block_transaction_count_by_number(self, block_number)
				.map(|x| x.map(RpcU256::from))
				.map(to_value)
		})
	}

	/// Returns the number of uncles in a block with given hash.
	fn block_uncles_count_by_hash(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params::<(RpcH256,)>(params).and_then(|(hash,)| {
			Eth::block_uncles_count_by_hash(self, &hash.into())
				.map(|x| x.map(RpcU256::from))
				.map(to_value)
		})
	}

	/// Returns the number of uncles in a block with given block number.
	fn block_uncles_count_by_number(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params(params).and_then(|(block_number,)| {
			Eth::block_uncles_count_by_number(self, block_number)
				.map(|x| x.map(RpcU256::from))
				.map(to_value)
		})
	}

	/// Returns the code at given address at given time (block number).
	fn code_at(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params_default_second::<RpcH160>(params).and_then(|(address, block_number,)| {
			Eth::code_at(self, &address.into(), block_number)
				.map(Bytes::from).map(to_value)
		})
	}

	/// Sends signed transaction.
	fn send_raw_transaction(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params::<(Bytes,)>(params).and_then(|(raw,)| {
			let raw = raw.to_vec();
			match UntrustedRlp::new(&raw).as_val() {
				Ok(signed_transaction) => Eth::send_raw_transaction(self, signed_transaction)
					.map(RpcH256::from).map(to_value),
				Err(_) => Ok(to_value(RpcH256::from(H256::from(0)))),
			}
		})
	}

	/// Call contract.
	fn call(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params_default_second::<CallRequest>(params).and_then(|(req, block_number)| {
			Eth::call(self, req, block_number).map(Bytes).map(to_value)
		})
	}

	/// Estimate gas needed for execution of given contract.
	fn estimate_gas(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params_default_second::<CallRequest>(params).and_then(|(req, block_number)| {
			Eth::estimate_gas(self, req, block_number).map(RpcU256::from).map(to_value)
		})
	}

	/// Get transaction by its hash.
	fn transaction_by_hash(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params::<(RpcH256,)>(params).and_then(|(hash,)| {
			Eth::transaction_by_hash(self, &hash.into()).map(to_value)
		})
	}

	/// Returns transaction at given block hash and index.
	fn transaction_by_block_hash_and_index(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params::<(RpcH256, Index)>(params).and_then(|(hash, index)| {
			Eth::transaction_by_block_hash_and_index(self, &hash.into(), index.value())
				.map(to_value)
		})
	}

	/// Returns transaction by given block number and index.
	fn transaction_by_block_number_and_index(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params::<(BlockNumber, Index)>(params).and_then(|(num, index)| {
			Eth::transaction_by_block_number_and_index(self, num, index.value())
				.map(to_value)
		})
	}

	/// Returns transaction receipt.
	fn transaction_receipt(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params::<(RpcH256,)>(params).and_then(|(hash,)| {
			Eth::transaction_receipt(self, &hash.into()).map(to_value)
		})
	}

	/// Returns an uncles at given block and index.
	fn uncle_by_block_hash_and_index(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params::<(RpcH256, Index)>(params).and_then(|(hash, index)| {
			Eth::uncle_by_block_hash_and_index(self, &hash.into(), index.value()).map(to_value)
		})
	}

	/// Returns an uncles at given block and index.
	fn uncle_by_block_number_and_index(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params::<(BlockNumber, Index)>(params).and_then(|(num, index)| {
			Eth::uncle_by_block_number_and_index(self, num, index.value())
				.map(to_value)
		})
	}

	/// Returns available compilers.
	fn compilers(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		try!(expect_no_params(params));

		Eth::compilers(self).map(to_value)
	}

	/// Compiles lll code.
	fn compile_lll(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params::<(String,)>(params).and_then(|(code,)| {
			Eth::compile_lll(self, code).map(Bytes).map(to_value)
		})
	}

	/// Compiles solidity.
	fn compile_solidity(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params::<(String,)>(params).and_then(|(code,)| {
			Eth::compile_solidity(self, code).map(Bytes).map(to_value)
		})
	}

	/// Compiles serpent.
	fn compile_serpent(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params::<(String,)>(params).and_then(|(code,)| {
			Eth::compile_serpent(self, code).map(Bytes).map(to_value)
		})
	}

	/// Returns logs matching given filter object.
	fn logs(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params::<(Filter,)>(params).and_then(|(filter,)| {
			Eth::logs(self, filter).map(to_value)
		})
	}

	/// Returns the hash of the current block, the seedHash, and the boundary condition to be met.
	fn work(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		let no_new_work_timeout = from_params::<(u64,)>(params).ok()
			.and_then(|(val,)| if val == 0 { None } else { Some(val) });

		match try!(Eth::work(self, no_new_work_timeout)) {
			(pow_hash, seed_hash, target, Some(number)) =>
				Ok(to_value((RpcH256::from(pow_hash), RpcH256::from(seed_hash), RpcH256::from(target), RpcU256::from(number)))),
			(pow_hash, seed_hash, target, None) =>
				Ok(to_value((RpcH256::from(pow_hash), RpcH256::from(seed_hash), RpcH256::from(target)))),
		}
	}

	/// Used for submitting a proof-of-work solution.
	fn submit_work(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params::<(RpcH64, RpcH256, RpcH256)>(params).and_then(|(nonce, pow_hash, mix_hash)| {
			Eth::submit_work(self, nonce.into(), pow_hash.into(), mix_hash.into()).map(to_value)
		})
	}

	/// Used for submitting mining hashrate.
	fn submit_hashrate(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params::<(RpcU256, RpcH256)>(params).and_then(|(rate, id)| {
			Eth::submit_hashrate(self, rate.into(), id.into()).map(to_value)
		})
	}
}

impl<T: EthFilter> EthFilterRpc for T {
	fn new_filter(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		from_params::<(Filter,)>(params).and_then(|(filter,)| {
			EthFilter::new_filter(self, filter).map(RpcU256::from).map(to_value)
		})
	}

	fn new_block_filter(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		try!(expect_no_params(params));

		EthFilter::new_block_filter(self).map(RpcU256::from).map(to_value)
	}

	fn new_pending_transaction_filter(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		try!(expect_no_params(params));

		EthFilter::new_pending_transaction_filter(self).map(RpcU256::from).map(to_value)
	}

	fn filter_changes(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params::<(Index,)>(params).and_then(|(index,)| {
			EthFilter::filter_changes(self, index.value()).map(|changes| match changes {
				FilterChanges::Blocks(hashes) | FilterChanges::Transactions(hashes) => to_value(hashes),
				FilterChanges::Logs(logs) => to_value(logs),
				FilterChanges::Invalid => to_value(&[] as &[Value]),
			})
		})
	}

	fn filter_logs(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params::<(Index,)>(params).and_then(|(index,)| {
			EthFilter::filter_logs(self, index.value()).map(to_value)
		})
	}

	fn uninstall_filter(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());

		from_params::<(Index,)>(params).and_then(|(index,)| {
			EthFilter::uninstall_filter(self, index.value()).map(to_value)
		})
	}
}