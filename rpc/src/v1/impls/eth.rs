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

//! Eth rpc implementation.

extern crate ethash;

use std::io::{Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Instant, Duration};
use std::sync::{Arc, Weak};
use time::get_time;
use ethsync::{SyncProvider, SyncState};
use ethcore::miner::{MinerService, ExternalMinerService};
use jsonrpc_core::*;
use util::{H256, Address, FixedHash, U256, H64, Uint};
use util::sha3::*;
use util::{FromHex, Mutex};
use rlp;
use ethcore::account_provider::AccountProvider;
use ethcore::client::{MiningBlockChainClient, BlockID, TransactionID, UncleID};
use ethcore::header::Header as BlockHeader;
use ethcore::block::IsBlock;
use ethcore::views::*;
use ethcore::ethereum::Ethash;
use ethcore::transaction::{Transaction as EthTransaction, SignedTransaction, Action};
use ethcore::log_entry::LogEntry;
use ethcore::filter::Filter as EthcoreFilter;
use self::ethash::SeedHashCompute;
use v1::traits::Eth;
use v1::types::{Block, BlockTransactions, BlockNumber, Bytes, SyncStatus, SyncInfo, Transaction, CallRequest, Filter, Log, Receipt};
use v1::helpers::{CallRequest as CRequest, errors};
use v1::helpers::dispatch::{default_gas_price, dispatch_transaction};

/// Eth RPC options
pub struct EthClientOptions {
	/// Returns receipt from pending blocks
	pub allow_pending_receipt_query: bool,
	/// Send additional block number when asking for work
	pub send_block_number_in_get_work: bool,
}

impl Default for EthClientOptions {
	fn default() -> Self {
		EthClientOptions {
			allow_pending_receipt_query: true,
			send_block_number_in_get_work: true,
		}
	}
}

/// Eth rpc implementation.
pub struct EthClient<C, S: ?Sized, M, EM> where
	C: MiningBlockChainClient,
	S: SyncProvider,
	M: MinerService,
	EM: ExternalMinerService {

	client: Weak<C>,
	sync: Weak<S>,
	accounts: Weak<AccountProvider>,
	miner: Weak<M>,
	external_miner: Arc<EM>,
	seed_compute: Mutex<SeedHashCompute>,
	options: EthClientOptions,
}

impl<C, S: ?Sized, M, EM> EthClient<C, S, M, EM> where
	C: MiningBlockChainClient,
	S: SyncProvider,
	M: MinerService,
	EM: ExternalMinerService {

	/// Creates new EthClient.
	pub fn new(client: &Arc<C>, sync: &Arc<S>, accounts: &Arc<AccountProvider>, miner: &Arc<M>, em: &Arc<EM>, options: EthClientOptions)
		-> EthClient<C, S, M, EM> {
		EthClient {
			client: Arc::downgrade(client),
			sync: Arc::downgrade(sync),
			miner: Arc::downgrade(miner),
			accounts: Arc::downgrade(accounts),
			external_miner: em.clone(),
			seed_compute: Mutex::new(SeedHashCompute::new()),
			options: options,
		}
	}

	fn block(&self, id: BlockID, include_txs: bool) -> Result<Option<Block>, Error> {
		let client = take_weak!(self.client);
		match (client.block(id.clone()), client.block_total_difficulty(id)) {
			(Some(bytes), Some(total_difficulty)) => {
				let block_view = BlockView::new(&bytes);
				let view = block_view.header_view();
				Ok(Some(Block {
					hash: Some(view.sha3().into()),
					size: Some(bytes.len().into()),
					parent_hash: view.parent_hash().into(),
					uncles_hash: view.uncles_hash().into(),
					author: view.author().into(),
					miner: view.author().into(),
					state_root: view.state_root().into(),
					transactions_root: view.transactions_root().into(),
					receipts_root: view.receipts_root().into(),
					number: Some(view.number().into()),
					gas_used: view.gas_used().into(),
					gas_limit: view.gas_limit().into(),
					logs_bloom: view.log_bloom().into(),
					timestamp: view.timestamp().into(),
					difficulty: view.difficulty().into(),
					total_difficulty: total_difficulty.into(),
					seal_fields: view.seal().into_iter().map(|f| rlp::decode(&f)).map(Bytes::new).collect(),
					uncles: block_view.uncle_hashes().into_iter().map(Into::into).collect(),
					transactions: match include_txs {
						true => BlockTransactions::Full(block_view.localized_transactions().into_iter().map(Into::into).collect()),
						false => BlockTransactions::Hashes(block_view.transaction_hashes().into_iter().map(Into::into).collect()),
					},
					extra_data: Bytes::new(view.extra_data())
				}))
			},
			_ => Ok(None)
		}
	}

	fn transaction(&self, id: TransactionID) -> Result<Option<Transaction>, Error> {
		Ok(take_weak!(self.client).transaction(id).map(Into::into))
	}

	fn uncle(&self, id: UncleID) -> Result<Option<Block>, Error> {
		let client = take_weak!(self.client);
		let uncle: BlockHeader = match client.uncle(id) {
			Some(rlp) => rlp::decode(&rlp),
			None => { return Ok(None); }
		};
		let parent_difficulty = match client.block_total_difficulty(BlockID::Hash(uncle.parent_hash().clone())) {
			Some(difficulty) => difficulty,
			None => { return Ok(None); }
		};

		Ok(Some(Block {
			hash: Some(uncle.hash().into()),
			size: None,
			parent_hash: uncle.parent_hash().clone().into(),
			uncles_hash: uncle.uncles_hash().clone().into(),
			author: uncle.author().clone().into(),
			miner: uncle.author().clone().into(),
			state_root: uncle.state_root().clone().into(),
			transactions_root: uncle.transactions_root().clone().into(),
			number: Some(uncle.number().into()),
			gas_used: uncle.gas_used().clone().into(),
			gas_limit: uncle.gas_limit().clone().into(),
			logs_bloom: uncle.log_bloom().clone().into(),
			timestamp: uncle.timestamp().into(),
			difficulty: uncle.difficulty().clone().into(),
			total_difficulty: (uncle.difficulty().clone() + parent_difficulty).into(),
			receipts_root: uncle.receipts_root().clone().into(),
			extra_data: uncle.extra_data().clone().into(),
			seal_fields: uncle.seal().clone().into_iter().map(|f| rlp::decode(&f)).map(Bytes::new).collect(),
			uncles: vec![],
			transactions: BlockTransactions::Hashes(vec![]),
		}))
	}

	fn sign_call(&self, request: CRequest) -> Result<SignedTransaction, Error> {
		let (client, miner) = (take_weak!(self.client), take_weak!(self.miner));
		let from = request.from.unwrap_or(Address::zero());
		Ok(EthTransaction {
			nonce: request.nonce.unwrap_or_else(|| client.latest_nonce(&from)),
			action: request.to.map_or(Action::Create, Action::Call),
			gas: request.gas.unwrap_or(U256::from(50_000_000)),
			gas_price: request.gas_price.unwrap_or_else(|| default_gas_price(&*client, &*miner)),
			value: request.value.unwrap_or_else(U256::zero),
			data: request.data.map_or_else(Vec::new, |d| d.to_vec())
		}.fake_sign(from))
	}
}

pub fn pending_logs<M>(miner: &M, filter: &EthcoreFilter) -> Vec<Log> where M: MinerService {
	let receipts = miner.pending_receipts();

	let pending_logs = receipts.into_iter()
		.flat_map(|(hash, r)| r.logs.into_iter().map(|l| (hash.clone(), l)).collect::<Vec<(H256, LogEntry)>>())
		.collect::<Vec<(H256, LogEntry)>>();

	let result = pending_logs.into_iter()
		.filter(|pair| filter.matches(&pair.1))
		.map(|pair| {
			let mut log = Log::from(pair.1);
			log.transaction_hash = Some(pair.0.into());
			log
		})
		.collect();

	result
}

const MAX_QUEUE_SIZE_TO_MINE_ON: usize = 4;	// because uncles go back 6.

#[cfg(windows)]
static SOLC: &'static str = "solc.exe";

#[cfg(not(windows))]
static SOLC: &'static str = "solc";

impl<C, S: ?Sized, M, EM> Eth for EthClient<C, S, M, EM> where
	C: MiningBlockChainClient + 'static,
	S: SyncProvider + 'static,
	M: MinerService + 'static,
	EM: ExternalMinerService + 'static {

	fn active(&self) -> Result<(), Error> {
		// TODO: only call every 30s at most.
		take_weak!(self.client).keep_alive();
		Ok(())
	}

	fn protocol_version(&self) -> Result<u32, Error> {
		Ok(take_weak!(self.sync).status().protocol_version as u32)
	}

	fn syncing(&self) -> Result<SyncStatus, Error> {
		let status = take_weak!(self.sync).status();
		let res = match status.state {
			SyncState::Idle => SyncStatus::None,
			SyncState::Waiting | SyncState::Blocks | SyncState::NewBlocks | SyncState::ChainHead
				| SyncState::SnapshotManifest | SyncState::SnapshotData | SyncState::SnapshotWaiting => {
				let current_block = U256::from(take_weak!(self.client).chain_info().best_block_number);
				let highest_block = U256::from(status.highest_block_number.unwrap_or(status.start_block_number));

				if highest_block > current_block + U256::from(6) {
					let info = SyncInfo {
						starting_block: status.start_block_number.into(),
						current_block: current_block.into(),
						highest_block: highest_block.into(),
					};
					SyncStatus::Info(info)
				} else {
					SyncStatus::None
				}
			}
		};

		Ok(res)
	}

	fn author(&self) -> Result<Address, Error> {
		Ok(take_weak!(self.miner).author())
	}

	fn is_mining(&self) -> Result<bool, Error> {
		Ok(take_weak!(self.miner).is_sealing())
	}

	fn hashrate(&self) -> Result<U256, Error> {
		Ok(self.external_miner.hashrate())
	}

	fn gas_price(&self) -> Result<U256, Error> {
		let (client, miner) = (take_weak!(self.client), take_weak!(self.miner));
		Ok(default_gas_price(&*client, &*miner))
	}

	fn accounts(&self) -> Result<Vec<Address>, Error> {
		let store = take_weak!(self.accounts);
		let accounts = try!(store.accounts().map_err(|e| errors::internal("Could not fetch accounts.", e)));
		Ok(accounts)
	}

	fn block_number(&self) -> Result<u64, Error> {
		Ok(take_weak!(self.client).chain_info().best_block_number)
	}

	fn balance(&self, address: &Address, at: BlockNumber) -> Result<U256, Error> {
		match at {
			BlockNumber::Pending => Ok(take_weak!(self.miner).balance(&*take_weak!(self.client), &address)),
			id => take_weak!(self.client).balance(&address, id.into())
				.ok_or_else(errors::state_pruned),
		}
	}

	fn storage_at(&self, address: &Address, key: &H256, at: BlockNumber) -> Result<H256, Error> {
		match at {
			BlockNumber::Pending => Ok(take_weak!(self.miner).storage_at(&*take_weak!(self.client), address, key)),
			id => take_weak!(self.client).storage_at(address, key, id.into())
				.ok_or_else(errors::state_pruned),
		}
	}

	fn transaction_count(&self, address: &Address, at: BlockNumber) -> Result<U256, Error> {
		match at {
			BlockNumber::Pending => Ok(take_weak!(self.miner).nonce(&*take_weak!(self.client), &address)),
			id => take_weak!(self.client).nonce(&address, id.into())
				.ok_or_else(errors::state_pruned),
		}
	}

	fn block_transaction_count_by_hash(&self, hash: &H256) -> Result<Option<usize>, Error> {
		Ok(take_weak!(self.client)
			.block(BlockID::Hash(hash.clone()))
			.map(|bytes| BlockView::new(&bytes).transactions_count()))
	}

	fn block_transaction_count_by_number(&self, number: BlockNumber) -> Result<Option<usize>, Error> {
		match number {
			BlockNumber::Pending => Ok(Some(take_weak!(self.miner).status().transactions_in_pending_block)),
			number => Ok(take_weak!(self.client).block(number.into())
				.map(|bytes| BlockView::new(&bytes).transactions_count()))
		}
	}

	fn block_uncles_count_by_hash(&self, hash: &H256) -> Result<Option<usize>, Error> {
		Ok(take_weak!(self.client)
			.block(BlockID::Hash(hash.clone()))
			.map(|bytes| BlockView::new(&bytes).uncles_count()))
	}

	fn block_uncles_count_by_number(&self, number: BlockNumber) -> Result<Option<usize>, Error> {
		match number {
			BlockNumber::Pending => Ok(Some(0)),
			number => Ok(take_weak!(self.client).block(number.into())
				.map(|bytes| BlockView::new(&bytes).uncles_count()))
		}
	}

	fn code_at(&self, address: &Address, at: BlockNumber) -> Result<Vec<u8>, Error> {
		match at {
			BlockNumber::Pending => Ok(take_weak!(self.miner).code(&*take_weak!(self.client), address).unwrap_or_else(Vec::new)),
			number => match take_weak!(self.client).code(address, number.into()) {
				Some(code) => Ok(code.unwrap_or_else(Vec::new)),
				None => Err(errors::state_pruned()),
			}
		}
	}

	fn block_by_hash(&self, hash: &H256, include_txs: bool) -> Result<Option<Block>, Error> {
		self.block(BlockID::Hash(hash.clone()), include_txs)
	}

	fn block_by_number(&self, number: BlockNumber, include_txs: bool) -> Result<Option<Block>, Error> {
		self.block(number.into(), include_txs)
	}

	fn transaction_by_hash(&self, hash: &H256) -> Result<Option<Transaction>, Error> {
		let miner = take_weak!(self.miner);
		match miner.transaction(&hash) {
			Some(pending_tx) => Ok(Some(pending_tx.into())),
			None => self.transaction(TransactionID::Hash(hash.clone()))
		}
	}

	fn transaction_by_block_hash_and_index(&self, hash: &H256, index: usize) -> Result<Option<Transaction>, Error> {
		self.transaction(TransactionID::Location(BlockID::Hash(hash.clone()), index))
	}

	fn transaction_by_block_number_and_index(&self, number: BlockNumber, index: usize) -> Result<Option<Transaction>, Error> {
		self.transaction(TransactionID::Location(number.into(), index))
	}

	fn transaction_receipt(&self, hash: &H256) -> Result<Option<Receipt>, Error> {
		let miner = take_weak!(self.miner);

		match (miner.pending_receipt(hash), self.options.allow_pending_receipt_query) {
			(Some(receipt), true) => Ok(Some(receipt.into())),
			_ => {
				let client = take_weak!(self.client);
				let receipt = client.transaction_receipt(TransactionID::Hash(hash.clone()));
				Ok(receipt.map(Into::into))
			}
		}
	}

	fn uncle_by_block_hash_and_index(&self, hash: &H256, index: usize) -> Result<Option<Block>, Error> {
		self.uncle(UncleID { block: BlockID::Hash(hash.clone()), position: index })
	}

	fn uncle_by_block_number_and_index(&self, number: BlockNumber, index: usize) -> Result<Option<Block>, Error> {
		self.uncle(UncleID { block: number.into(), position: index })
	}

	fn compilers(&self) -> Result<Vec<String>, Error> {
		let mut compilers = vec![];
		if Command::new(SOLC).output().is_ok() {
			compilers.push("solidity".to_owned())
		}

		Ok(compilers)
	}

	fn logs(&self, filter: Filter, limit: Option<usize>) -> Result<Vec<Log>, Error> {
		let include_pending = filter.to_block == Some(BlockNumber::Pending);
		let filter: EthcoreFilter = filter.into();
		let mut logs = take_weak!(self.client).logs(filter.clone(), limit)
			.into_iter()
			.map(From::from)
			.collect::<Vec<Log>>();

		if include_pending {
			let pending = pending_logs(&*take_weak!(self.miner), &filter);
			logs.extend(pending);
		}

		let len = logs.len();
		match limit {
			Some(limit) if len >= limit => {
				logs = logs.split_off(len - limit);
			}
			_ => {}
		}

		Ok(logs)
	}

	fn work(&self, no_new_work_timeout: Option<u64>)
		-> Result<(H256, H256, H256, Option<u64>), Error> {
		let client = take_weak!(self.client);
		let no_new_work_timeout = no_new_work_timeout.unwrap_or(0);

		// check if we're still syncing and return empty strings in that case
		{
			//TODO: check if initial sync is complete here
			//let sync = take_weak!(self.sync);
			if /*sync.status().state != SyncState::Idle ||*/ client.queue_info().total_queue_size() > MAX_QUEUE_SIZE_TO_MINE_ON {
				trace!(target: "miner", "Syncing. Cannot give any work.");
				return Err(errors::no_work());
			}

			// Otherwise spin until our submitted block has been included.
			let timeout = Instant::now() + Duration::from_millis(1000);
			while Instant::now() < timeout && client.queue_info().total_queue_size() > 0 {
				thread::sleep(Duration::from_millis(1));
			}
		}

		let miner = take_weak!(self.miner);
		if miner.author().is_zero() {
			warn!(target: "miner", "Cannot give work package - no author is configured. Use --author to configure!");
			return Err(errors::no_author())
		}
		miner.map_sealing_work(&*client, |b| {
			let pow_hash = b.hash();
			let target = Ethash::difficulty_to_boundary(b.block().header().difficulty());
			let number = b.block().header().number();
			let seed_hash = H256(self.seed_compute.lock().get_seedhash(number));

			if no_new_work_timeout > 0 && b.block().header().timestamp() + no_new_work_timeout < get_time().sec as u64 {
				Err(errors::no_new_work())
			} else if self.options.send_block_number_in_get_work {
				Ok((pow_hash, seed_hash, target, Some(number)))
			} else {
				Ok((pow_hash, seed_hash, target, None))
			}
		}).unwrap_or(Err(Error::internal_error()))	// no work found.
	}

	fn submit_work(&self, nonce: H64, pow_hash: H256, mix_hash: H256) -> Result<bool, Error> {
		let (client, miner) = (take_weak!(self.client), take_weak!(self.miner));
		let seal = vec![rlp::encode(&mix_hash).to_vec(), rlp::encode(&nonce).to_vec()];
		Ok(miner.submit_seal(&*client, pow_hash, seal).is_ok())
	}

	fn submit_hashrate(&self, rate: U256, id: H256) -> Result<bool, Error> {
		self.external_miner.submit_hashrate(rate, id);
		Ok(true)
	}

	fn send_raw_transaction(&self, transaction: SignedTransaction) -> Result<H256, Error> {
		dispatch_transaction(&*take_weak!(self.client), &*take_weak!(self.miner), transaction)
	}

	fn call(&self, request: CallRequest, at: BlockNumber) -> Result<Vec<u8>, Error> {
		let signed = try!(self.sign_call(request.into()));
		let r = match at {
			BlockNumber::Pending => take_weak!(self.miner).call(&*take_weak!(self.client), &signed, Default::default()),
			number => take_weak!(self.client).call(&signed, number.into(), Default::default()),
		};

		Ok(r.map(|e| e.output).unwrap_or(Vec::new()))
	}

	fn estimate_gas(&self, request: CallRequest, at: BlockNumber) -> Result<U256, Error> {
		let signed = try!(self.sign_call(request.into()));
		let r = match at {
			BlockNumber::Pending => take_weak!(self.miner).call(&*take_weak!(self.client), &signed, Default::default()),
			number => take_weak!(self.client).call(&signed, number.into(), Default::default()),
		};

		Ok(r.map(|res| res.gas_used + res.refunded).unwrap_or(0.into()))
	}

	fn compile_lll(&self, _code: String) -> Result<Vec<u8>, Error> {
		rpc_unimplemented!()
	}

	fn compile_serpent(&self, _code: String) -> Result<Vec<u8>, Error> {
		rpc_unimplemented!()
	}

	fn compile_solidity(&self, code: String) -> Result<Vec<u8>, Error> {
		let maybe_child = Command::new(SOLC)
			.arg("--bin")
			.arg("--optimize")
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::null())
			.spawn();

		maybe_child
			.map_err(errors::compilation)
			.and_then(|mut child| {
				try!(child.stdin.as_mut()
					.expect("we called child.stdin(Stdio::piped()) before spawn; qed")
					.write_all(code.as_bytes())
					.map_err(errors::compilation));
				let output = try!(child.wait_with_output().map_err(errors::compilation));

				let s = String::from_utf8_lossy(&output.stdout);
				if let Some(hex) = s.lines().skip_while(|ref l| !l.contains("Binary")).skip(1).next() {
					Ok(hex.from_hex().unwrap_or(vec![]))
				} else {
					Err(errors::compilation("Unexpected output."))
				}
			})
	}
}
