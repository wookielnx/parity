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

//! Snapshot creation, restoration, and network service.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use account_db::{AccountDB, AccountDBMut};
use blockchain::{BlockChain, BlockProvider};
use engines::Engine;
use ids::BlockID;
use views::BlockView;

use util::{Bytes, Hashable, HashDB, snappy};
use util::memorydb::MemoryDB;
use util::Mutex;
use util::hash::{FixedHash, H256};
use util::journaldb::{self, Algorithm, JournalDB};
use util::kvdb::Database;
use util::trie::{TrieDB, TrieDBMut, Trie, TrieMut};
use util::sha3::SHA3_NULL_RLP;
use rlp::{RlpStream, Stream, UntrustedRlp, View, Compressible, RlpType};

use self::account::Account;
use self::block::AbridgedBlock;
use self::io::SnapshotWriter;

use crossbeam::{scope, ScopedJoinHandle};
use rand::{Rng, OsRng};

pub use self::error::Error;

pub use self::service::{Service, DatabaseRestore};
pub use self::traits::{SnapshotService, RemoteSnapshotService};
pub use self::watcher::Watcher;
pub use types::snapshot_manifest::ManifestData;
pub use types::restoration_status::RestorationStatus;

pub mod io;
pub mod service;

mod account;
mod block;
mod error;
mod watcher;

#[cfg(test)]
mod tests;

mod traits {
	#![allow(dead_code, unused_assignments, unused_variables, missing_docs)] // codegen issues
	include!(concat!(env!("OUT_DIR"), "/snapshot_service_trait.rs"));
}

// Try to have chunks be around 4MB (before compression)
const PREFERRED_CHUNK_SIZE: usize = 4 * 1024 * 1024;

// How many blocks to include in a snapshot, starting from the head of the chain.
const SNAPSHOT_BLOCKS: u64 = 30000;

/// A progress indicator for snapshots.
#[derive(Debug, Default)]
pub struct Progress {
	accounts: AtomicUsize,
	blocks: AtomicUsize,
	size: AtomicUsize, // Todo [rob] use Atomicu64 when it stabilizes.
	done: AtomicBool,
}

impl Progress {
	/// Reset the progress.
	pub fn reset(&self) {
		self.accounts.store(0, Ordering::Release);
		self.blocks.store(0, Ordering::Release);
		self.size.store(0, Ordering::Release);

		// atomic fence here to ensure the others are written first?
		// logs might very rarely get polluted if not.
		self.done.store(false, Ordering::Release);
	}

	/// Get the number of accounts snapshotted thus far.
	pub fn accounts(&self) -> usize { self.accounts.load(Ordering::Acquire) }

	/// Get the number of blocks snapshotted thus far.
	pub fn blocks(&self) -> usize { self.blocks.load(Ordering::Acquire) }

	/// Get the written size of the snapshot in bytes.
	pub fn size(&self) -> usize { self.size.load(Ordering::Acquire) }

	/// Whether the snapshot is complete.
	pub fn done(&self) -> bool  { self.done.load(Ordering::Acquire) }

}
/// Take a snapshot using the given blockchain, starting block hash, and database, writing into the given writer.
pub fn take_snapshot<W: SnapshotWriter + Send>(
	chain: &BlockChain,
	block_at: H256,
	state_db: &HashDB,
	writer: W,
	p: &Progress
) -> Result<(), Error> {
	let start_header = try!(chain.block_header(&block_at)
		.ok_or(Error::InvalidStartingBlock(BlockID::Hash(block_at))));
	let state_root = start_header.state_root();
	let number = start_header.number();

	info!("Taking snapshot starting at block {}", number);

	let writer = Mutex::new(writer);
	let (state_hashes, block_hashes) = try!(scope(|scope| {
		let block_guard = scope.spawn(|| chunk_blocks(chain, (number, block_at), &writer, p));
		let state_res = chunk_state(state_db, state_root, &writer, p);

		state_res.and_then(|state_hashes| {
			block_guard.join().map(|block_hashes| (state_hashes, block_hashes))
		})
	}));

	info!("produced {} state chunks and {} block chunks.", state_hashes.len(), block_hashes.len());

	let manifest_data = ManifestData {
		state_hashes: state_hashes,
		block_hashes: block_hashes,
		state_root: *state_root,
		block_number: number,
		block_hash: block_at,
	};

	try!(writer.into_inner().finish(manifest_data));

	p.done.store(true, Ordering::SeqCst);

	Ok(())
}

/// Used to build block chunks.
struct BlockChunker<'a> {
	chain: &'a BlockChain,
	// block, receipt rlp pairs.
	rlps: VecDeque<Bytes>,
	current_hash: H256,
	hashes: Vec<H256>,
	snappy_buffer: Vec<u8>,
	writer: &'a Mutex<SnapshotWriter + 'a>,
	progress: &'a Progress,
}

impl<'a> BlockChunker<'a> {
	// Repeatedly fill the buffers and writes out chunks, moving backwards from starting block hash.
	// Loops until we reach the first desired block, and writes out the remainder.
	fn chunk_all(&mut self, first_hash: H256) -> Result<(), Error> {
		let mut loaded_size = 0;

		while self.current_hash != first_hash {
			let (block, receipts) = try!(self.chain.block(&self.current_hash)
				.and_then(|b| self.chain.block_receipts(&self.current_hash).map(|r| (b, r)))
				.ok_or(Error::BlockNotFound(self.current_hash)));

			let view = BlockView::new(&block);
			let abridged_rlp = AbridgedBlock::from_block_view(&view).into_inner();

			let pair = {
				let mut pair_stream = RlpStream::new_list(2);
				pair_stream.append_raw(&abridged_rlp, 1).append(&receipts);
				pair_stream.out()
			};

			let new_loaded_size = loaded_size + pair.len();

			// cut off the chunk if too large.

			if new_loaded_size > PREFERRED_CHUNK_SIZE {
				try!(self.write_chunk());
				loaded_size = pair.len();
			} else {
				loaded_size = new_loaded_size;
			}

			self.rlps.push_front(pair);
			self.current_hash = view.header_view().parent_hash();
		}

		if loaded_size != 0 {
			// we don't store the first block, so once we get to this point,
			// the "first" block will be first_number + 1.
			try!(self.write_chunk());
		}

		Ok(())
	}

	// write out the data in the buffers to a chunk on disk
	//
	// we preface each chunk with the parent of the first block's details.
	fn write_chunk(&mut self) -> Result<(), Error> {
		// since the block we're inspecting now doesn't go into the
		// chunk if it's too large, the current hash is the parent hash
		// for the first block in that chunk.
		let parent_hash = self.current_hash;

		trace!(target: "snapshot", "prepared block chunk with {} blocks", self.rlps.len());
		let (parent_number, parent_details) = try!(self.chain.block_number(&parent_hash)
			.and_then(|n| self.chain.block_details(&parent_hash).map(|d| (n, d)))
			.ok_or(Error::BlockNotFound(parent_hash)));

		let parent_total_difficulty = parent_details.total_difficulty;

		let num_entries = self.rlps.len();
		let mut rlp_stream = RlpStream::new_list(3 + num_entries);
		rlp_stream.append(&parent_number).append(&parent_hash).append(&parent_total_difficulty);

		for pair in self.rlps.drain(..) {
			rlp_stream.append_raw(&pair, 1);
		}

		let raw_data = rlp_stream.out();

		let size = snappy::compress_into(&raw_data, &mut self.snappy_buffer);
		let compressed = &self.snappy_buffer[..size];
		let hash = compressed.sha3();

		try!(self.writer.lock().write_block_chunk(hash, compressed));
		trace!(target: "snapshot", "wrote block chunk. hash: {}, size: {}, uncompressed size: {}", hash.hex(), size, raw_data.len());

		self.progress.size.fetch_add(size, Ordering::SeqCst);
		self.progress.blocks.fetch_add(num_entries, Ordering::SeqCst);

		self.hashes.push(hash);
		Ok(())
	}
}

/// Create and write out all block chunks to disk, returning a vector of all
/// the hashes of block chunks created.
///
/// The path parameter is the directory to store the block chunks in.
/// This function assumes the directory exists already.
/// Returns a list of chunk hashes, with the first having the blocks furthest from the genesis.
pub fn chunk_blocks<'a>(chain: &'a BlockChain, start_block_info: (u64, H256), writer: &Mutex<SnapshotWriter + 'a>, progress: &'a Progress) -> Result<Vec<H256>, Error> {
	let (start_number, start_hash) = start_block_info;

	let first_hash = if start_number < SNAPSHOT_BLOCKS {
		// use the genesis hash.
		chain.genesis_hash()
	} else {
		let first_num = start_number - SNAPSHOT_BLOCKS;
		try!(chain.block_hash(first_num).ok_or(Error::IncompleteChain))
	};

	let mut chunker = BlockChunker {
		chain: chain,
		rlps: VecDeque::new(),
		current_hash: start_hash,
		hashes: Vec::new(),
		snappy_buffer: vec![0; snappy::max_compressed_len(PREFERRED_CHUNK_SIZE)],
		writer: writer,
		progress: progress,
	};

	try!(chunker.chunk_all(first_hash));

	Ok(chunker.hashes)
}

/// State trie chunker.
struct StateChunker<'a> {
	hashes: Vec<H256>,
	rlps: Vec<Bytes>,
	cur_size: usize,
	snappy_buffer: Vec<u8>,
	writer: &'a Mutex<SnapshotWriter + 'a>,
	progress: &'a Progress,
}

impl<'a> StateChunker<'a> {
	// Push a key, value pair to be encoded.
	//
	// If the buffer is greater than the desired chunk size,
	// this will write out the data to disk.
	fn push(&mut self, account_hash: Bytes, data: Bytes) -> Result<(), Error> {
		let pair = {
			let mut stream = RlpStream::new_list(2);
			stream.append(&account_hash).append_raw(&data, 1);
			stream.out()
		};

		if self.cur_size + pair.len() >= PREFERRED_CHUNK_SIZE {
			try!(self.write_chunk());
		}

		self.cur_size += pair.len();
		self.rlps.push(pair);

		Ok(())
	}

	// Write out the buffer to disk, pushing the created chunk's hash to
	// the list.
	fn write_chunk(&mut self) -> Result<(), Error> {
		let num_entries = self.rlps.len();
		let mut stream = RlpStream::new_list(num_entries);
		for rlp in self.rlps.drain(..) {
			stream.append_raw(&rlp, 1);
		}

		let raw_data = stream.out();

		let compressed_size = snappy::compress_into(&raw_data, &mut self.snappy_buffer);
		let compressed = &self.snappy_buffer[..compressed_size];
		let hash = compressed.sha3();

		try!(self.writer.lock().write_state_chunk(hash, compressed));
		trace!(target: "snapshot", "wrote state chunk. size: {}, uncompressed size: {}", compressed_size, raw_data.len());

		self.progress.accounts.fetch_add(num_entries, Ordering::SeqCst);
		self.progress.size.fetch_add(compressed_size, Ordering::SeqCst);

		self.hashes.push(hash);
		self.cur_size = 0;

		Ok(())
	}
}

/// Walk the given state database starting from the given root,
/// creating chunks and writing them out.
///
/// Returns a list of hashes of chunks created, or any error it may
/// have encountered.
pub fn chunk_state<'a>(db: &HashDB, root: &H256, writer: &Mutex<SnapshotWriter + 'a>, progress: &'a Progress) -> Result<Vec<H256>, Error> {
	let account_trie = try!(TrieDB::new(db, &root));

	let mut chunker = StateChunker {
		hashes: Vec::new(),
		rlps: Vec::new(),
		cur_size: 0,
		snappy_buffer: vec![0; snappy::max_compressed_len(PREFERRED_CHUNK_SIZE)],
		writer: writer,
		progress: progress,
	};

	let mut used_code = HashSet::new();

	// account_key here is the address' hash.
	for (account_key, account_data) in account_trie.iter() {
		let account = Account::from_thin_rlp(account_data);
		let account_key_hash = H256::from_slice(&account_key);

		let account_db = AccountDB::from_hash(db, account_key_hash);

		let fat_rlp = try!(account.to_fat_rlp(&account_db, &mut used_code));
		let compressed_rlp = UntrustedRlp::new(&fat_rlp).compress(RlpType::Snapshot).to_vec();
		try!(chunker.push(account_key, compressed_rlp));
	}

	if chunker.cur_size != 0 {
		try!(chunker.write_chunk());
	}

	Ok(chunker.hashes)
}

/// Used to rebuild the state trie piece by piece.
pub struct StateRebuilder {
	db: Box<JournalDB>,
	state_root: H256,
	code_map: HashMap<H256, Bytes>, // maps code hashes to code itself.
	missing_code: HashMap<H256, Vec<H256>>, // maps code hashes to lists of accounts missing that code.
}

impl StateRebuilder {
	/// Create a new state rebuilder to write into the given backing DB.
	pub fn new(db: Arc<Database>, pruning: Algorithm) -> Self {
		StateRebuilder {
			db: journaldb::new(db.clone(), pruning, ::db::COL_STATE),
			state_root: SHA3_NULL_RLP,
			code_map: HashMap::new(),
			missing_code: HashMap::new(),
		}
	}

	/// Feed an uncompressed state chunk into the rebuilder.
	pub fn feed(&mut self, chunk: &[u8]) -> Result<(), ::error::Error> {
		let rlp = UntrustedRlp::new(chunk);
		let account_fat_rlps: Vec<_> = rlp.iter().map(|r| r.as_raw()).collect();
		let mut pairs = Vec::with_capacity(rlp.item_count());

		// initialize the pairs vector with empty values so we have slots to write into.
		pairs.resize(rlp.item_count(), (H256::new(), Vec::new()));

		let chunk_size = account_fat_rlps.len() / ::num_cpus::get() + 1;

		// new code contained within this chunk.
		let mut chunk_code = HashMap::new();

		// build account tries in parallel.
		// Todo [rob] keep a thread pool around so we don't do this per-chunk.
		try!(scope(|scope| {
			let mut handles = Vec::new();
			for (account_chunk, out_pairs_chunk) in account_fat_rlps.chunks(chunk_size).zip(pairs.chunks_mut(chunk_size)) {
				let code_map = &self.code_map;
				let handle: ScopedJoinHandle<Result<_, ::error::Error>> = scope.spawn(move || {
					let mut db = MemoryDB::new();
					let status = try!(rebuild_accounts(&mut db, account_chunk, out_pairs_chunk, code_map));

					trace!(target: "snapshot", "thread rebuilt {} account tries", account_chunk.len());
					Ok((db, status))
				});

				handles.push(handle);
			}

			// consolidate all edits into the main overlay.
			for handle in handles {
				let (thread_db, status): (MemoryDB, _) = try!(handle.join());
				self.db.consolidate(thread_db);

				chunk_code.extend(status.new_code);

				for (addr_hash, code_hash) in status.missing_code {
					self.missing_code.entry(code_hash).or_insert_with(Vec::new).push(addr_hash);
				}
			}

			Ok::<_, ::error::Error>(())
		}));

		// patch up all missing code. must be done after collecting all new missing code entries.
		for (code_hash, code) in chunk_code {
			for addr_hash in self.missing_code.remove(&code_hash).unwrap_or_else(Vec::new) {
				let mut db = AccountDBMut::from_hash(self.db.as_hashdb_mut(), addr_hash);
				db.emplace(code_hash, code.clone());
			}

			self.code_map.insert(code_hash, code);
		}


		// batch trie writes
		{
			let mut account_trie = if self.state_root != SHA3_NULL_RLP {
				try!(TrieDBMut::from_existing(self.db.as_hashdb_mut(), &mut self.state_root))
			} else {
				TrieDBMut::new(self.db.as_hashdb_mut(), &mut self.state_root)
			};

			for (hash, thin_rlp) in pairs {
				try!(account_trie.insert(&hash, &thin_rlp));
			}
		}

		let backing = self.db.backing().clone();
		let mut batch = backing.transaction();
		try!(self.db.inject(&mut batch));
		try!(backing.write(batch).map_err(::util::UtilError::SimpleString));
		trace!(target: "snapshot", "current state root: {:?}", self.state_root);
		Ok(())
	}

	/// Check for accounts missing code. Once all chunks have been fed, there should
	/// be none.
	pub fn check_missing(self) -> Result<(), Error> {
		let missing = self.missing_code.keys().cloned().collect::<Vec<_>>();
		match missing.is_empty() {
			true => Ok(()),
			false => Err(Error::MissingCode(missing)),
		}
	}

	/// Get the state root of the rebuilder.
	pub fn state_root(&self) -> H256 { self.state_root }
}

#[derive(Default)]
struct RebuiltStatus {
	new_code: Vec<(H256, Bytes)>, // new code that's become available.
	missing_code: Vec<(H256, H256)>, // accounts that are missing code.
}

// rebuild a set of accounts and their storage.
// returns
fn rebuild_accounts(
	db: &mut HashDB,
	account_chunk: &[&[u8]],
	out_chunk: &mut [(H256, Bytes)],
	code_map: &HashMap<H256, Bytes>
) -> Result<RebuiltStatus, ::error::Error>
{
	let mut status = RebuiltStatus::default();
	for (account_pair, out) in account_chunk.into_iter().zip(out_chunk) {
		let account_rlp = UntrustedRlp::new(account_pair);

		let hash: H256 = try!(account_rlp.val_at(0));
		let decompressed = try!(account_rlp.at(1)).decompress(RlpType::Snapshot);
		let fat_rlp = UntrustedRlp::new(&decompressed[..]);

		let thin_rlp = {
			let mut acct_db = AccountDBMut::from_hash(db, hash);

			// fill out the storage trie and code while decoding.
			let (acc, maybe_code) = try!(Account::from_fat_rlp(&mut acct_db, fat_rlp, code_map));

			let code_hash = acc.code_hash().clone();
			match maybe_code {
				Some(code) => status.new_code.push((code_hash, code)),
				None => {
					if code_hash != ::util::SHA3_EMPTY && !code_map.contains_key(&code_hash) {
						status.missing_code.push((hash, code_hash));
					}
				}
			}

			acc.to_thin_rlp()
		};

		*out = (hash, thin_rlp);
	}
	Ok(status)
}

/// Proportion of blocks which we will verify `PoW` for.
const POW_VERIFY_RATE: f32 = 0.02;

/// Rebuilds the blockchain from chunks.
///
/// Does basic verification for all blocks, but `PoW` verification for some.
/// Blocks must be fed in-order.
///
/// The first block in every chunk is disconnected from the last block in the
/// chunk before it, as chunks may be submitted out-of-order.
///
/// After all chunks have been submitted, we "glue" the chunks together.
pub struct BlockRebuilder {
	chain: BlockChain,
	db: Arc<Database>,
	rng: OsRng,
	disconnected: Vec<(u64, H256)>,
	best_number: u64,
}

impl BlockRebuilder {
	/// Create a new BlockRebuilder.
	pub fn new(chain: BlockChain, db: Arc<Database>, best_number: u64) -> Result<Self, ::error::Error> {
		Ok(BlockRebuilder {
			chain: chain,
			db: db,
			rng: try!(OsRng::new()),
			disconnected: Vec::new(),
			best_number: best_number,
		})
	}

	/// Feed the rebuilder an uncompressed block chunk.
	/// Returns the number of blocks fed or any errors.
	pub fn feed(&mut self, chunk: &[u8], engine: &Engine) -> Result<u64, ::error::Error> {
		use basic_types::Seal::With;
		use util::U256;

		let rlp = UntrustedRlp::new(chunk);
		let item_count = rlp.item_count();

		trace!(target: "snapshot", "restoring block chunk with {} blocks.", item_count - 2);

		// todo: assert here that these values are consistent with chunks being in order.
		let mut cur_number = try!(rlp.val_at::<u64>(0)) + 1;
		let mut parent_hash = try!(rlp.val_at::<H256>(1));
		let parent_total_difficulty = try!(rlp.val_at::<U256>(2));

		for idx in 3..item_count {
			let pair = try!(rlp.at(idx));
			let abridged_rlp = try!(pair.at(0)).as_raw().to_owned();
			let abridged_block = AbridgedBlock::from_raw(abridged_rlp);
			let receipts: Vec<::receipt::Receipt> = try!(pair.val_at(1));
			let block = try!(abridged_block.to_block(parent_hash, cur_number));
			let block_bytes = block.rlp_bytes(With);

			if self.rng.gen::<f32>() <= POW_VERIFY_RATE {
				try!(engine.verify_block_seal(&block.header))
			} else {
				try!(engine.verify_block_basic(&block.header, Some(&block_bytes)));
			}

			let is_best = cur_number == self.best_number;
			let mut batch = self.db.transaction();

			// special-case the first block in each chunk.
			if idx == 3 {
				if self.chain.insert_unordered_block(&mut batch, &block_bytes, receipts, Some(parent_total_difficulty), is_best, false) {
					self.disconnected.push((cur_number, block.header.hash()));
				}
			} else {
				self.chain.insert_unordered_block(&mut batch, &block_bytes, receipts, None, is_best, false);
			}
			self.db.write(batch).expect("Error writing to the DB");
			self.chain.commit();

			parent_hash = BlockView::new(&block_bytes).hash();
			cur_number += 1;
		}

		Ok(item_count as u64 - 3)
	}

	/// Glue together any disconnected chunks. To be called at the end.
	pub fn glue_chunks(self) {
		for (first_num, first_hash) in self.disconnected {
			let parent_num = first_num - 1;

			// check if the parent is even in the chain.
			// since we don't restore every single block in the chain,
			// the first block of the first chunks has nothing to connect to.
			if let Some(parent_hash) = self.chain.block_hash(parent_num) {
				// if so, add the child to it.
				self.chain.add_child(parent_hash, first_hash);
			}
		}
	}
}
