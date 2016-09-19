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

///
/// Blockchain downloader
///

use util::*;
use rlp::*;
use ethcore::views::{BlockView};
use ethcore::header::{BlockNumber, Header as BlockHeader};
use ethcore::client::{BlockStatus, BlockID, BlockImportError};
use ethcore::block::Block;
use ethcore::error::{ImportError, BlockError};
use sync_io::SyncIo;
use blocks::BlockCollection;

const MAX_HEADERS_TO_REQUEST: usize = 128;
const MAX_BODIES_TO_REQUEST: usize = 128;
const MAX_RECEPITS_TO_REQUEST: usize = 128;
const SUBCHAIN_SIZE: u64 = 256;
const MAX_ROUND_PARENTS: usize = 32;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
/// Downloader state
pub enum State {
	/// No active downloads.
	Idle,
	/// Downloading subchain heads
	ChainHead,
	/// Downloading blocks
	Blocks,
	/// Download is complete
	Complete,
}

/// Data that needs to be requested from a peer.
pub enum BlockRequest {
	Headers {
		start: H256,
		count: u64,
		skip: u64,
	},
	Bodies {
		hashes: Vec<H256>,
	},
	Receipts {
		hashes: Vec<H256>,
	},
}

#[derive(Eq, PartialEq, Debug)]
pub enum BlockDownloaderImportError {
	/// Imported data is rejected as invalid.
	Invalid,
	/// Imported data is valid but rejected cause the downloader does not need it.
	Useless
}

/// Block downloader strategy.
/// Manages state and block data for a block download process.
pub struct BlockDownloader {
	/// Downloader state
	state: State,
	/// Highest block number seen
	highest_block: Option<BlockNumber>,
	/// Downloaded blocks, holds `H`, `B` and `S`
	blocks: BlockCollection,
	/// Last impoted block number
	last_imported_block: BlockNumber,
	/// Last impoted block hash
	last_imported_hash: H256,
	/// Number of blocks imported this round
	imported_this_round: Option<usize>,
	/// Block parents imported this round (hash, parent)
	round_parents: VecDeque<(H256, H256)>,
	/// Do we need to download block recetips.
	download_receipts: bool,
	/// Sync up to the block with this hash.
	target_hash: Option<H256>,
}

impl BlockDownloader {
	/// Create a new instance of syncing strategy.
	pub fn new(sync_receipts: bool, start_hash: &H256, start_number: BlockNumber) -> BlockDownloader {
		BlockDownloader {
			state: State::Idle,
			highest_block: None,
			last_imported_block: start_number,
			last_imported_hash: start_hash.clone(),
			blocks: BlockCollection::new(sync_receipts),
			imported_this_round: None,
			round_parents: VecDeque::new(),
			download_receipts: sync_receipts,
			target_hash: None,
		}
	}

	/// Reset sync. Clear all local downloaded data.
	pub fn reset(&mut self) {
		self.blocks.clear();
		self.state = State::Idle;
	}

	/// Mark a block as known in the chain
	pub fn mark_as_known(&mut self, hash: &H256, number: BlockNumber) {
		if number == self.last_imported_block + 1 {
			self.last_imported_block = number;
			self.last_imported_hash = hash.clone();
		}
	}

	/// Check if download is complete
	pub fn is_complete(&self) -> bool {
		self.state == State::Complete
	}

	/// Check if particular block hash is being downloaded
	pub fn is_downloading(&self, hash: &H256) -> bool {
		self.blocks.is_downloading(hash)
	}

	/// Set starting sync block
	pub fn set_target(&mut self, hash: &H256) {
		self.target_hash = Some(hash.clone());
	}

	/// Set starting sync block
	pub fn _set_start(&mut self, hash: &H256, number: BlockNumber) {
		self.last_imported_hash = hash.clone();
		self.last_imported_block = number;
	}

	/// Unmark header as being downloaded.
	pub fn clear_header_download(&mut self, hash: &H256) {
		self.blocks.clear_header_download(hash)
	}

	/// Unmark block body as being downloaded.
	pub fn clear_body_download(&mut self, hashes: &[H256]) {
		self.blocks.clear_body_download(hashes)
	}

	/// Unmark block receipt as being downloaded.
	pub fn clear_receipt_download(&mut self, hashes: &[H256]) {
		self.blocks.clear_receipt_download(hashes)
	}
	/// Reset collection for a new sync round with given subchain block hashes.
	pub fn reset_to(&mut self, hashes: Vec<H256>) {
		self.reset();
		self.blocks.reset_to(hashes);
	}

	/// Returns used heap memory size.
	pub fn heap_size(&self) -> usize {
		self.blocks.heap_size() + self.round_parents.heap_size_of_children()
	}

	/// Returns best imported block number.
	pub fn last_imported_block_number(&self) -> BlockNumber {
		self.last_imported_block
	}

	/// Add new block headers.
	pub fn import_headers(&mut self, io: &mut SyncIo, r: &UntrustedRlp, expected_hash: Option<H256>) -> Result<(), BlockDownloaderImportError> {
		let item_count = r.item_count();
		if self.state == State::Idle {
			trace!(target: "sync", "Ignored unexpected block headers");
			return Ok(())
		}
		if item_count == 0 && (self.state == State::Blocks) {
			return Err(BlockDownloaderImportError::Invalid);
		}

		let mut headers = Vec::new();
		let mut hashes = Vec::new();
		let mut valid_response = item_count == 0; //empty response is valid
		for i in 0..item_count {
			let info: BlockHeader = try!(r.val_at(i).map_err(|e| {
				trace!(target: "sync", "Error decoding block header RLP: {:?}", e);
				BlockDownloaderImportError::Invalid
			}));
			let number = BlockNumber::from(info.number());
			// Check if any of the headers matches the hash we requested
			if !valid_response {
				if let Some(expected) = expected_hash {
					valid_response = expected == info.hash()
				}
			}
			if self.blocks.contains(&info.hash()) {
				trace!(target: "sync", "Skipping existing block header {} ({:?})", number, info.hash());
				continue;
			}

			if self.highest_block == None || number > self.highest_block.unwrap() {
				self.highest_block = Some(number);
			}
			let hash = info.hash();
			let hdr = try!(r.at(i).map_err(|e| {
				trace!(target: "sync", "Error decoding block header RLP: {:?}", e);
				BlockDownloaderImportError::Invalid
			}));
			match io.chain().block_status(BlockID::Hash(hash.clone())) {
				BlockStatus::InChain | BlockStatus::Queued => {
					match self.state {
						State::Blocks => trace!(target: "sync", "Header already in chain {} ({})", number, hash),
						_ => trace!(target: "sync", "Header already in chain {} ({}), state = {:?}", number, hash, self.state),
					}
					headers.push(hdr.as_raw().to_vec());
					hashes.push(hash);
				},
				BlockStatus::Bad => {
					return Err(BlockDownloaderImportError::Invalid);
				},
				BlockStatus::Unknown => {
					headers.push(hdr.as_raw().to_vec());
					hashes.push(hash);
				}
			}
		}

		// Disable the peer for this syncing round if it gives invalid chain
		if !valid_response {
			trace!(target: "sync", "Invalid headers response");
			return Err(BlockDownloaderImportError::Invalid);
		}

		match self.state {
			State::ChainHead => {
				if headers.is_empty() {
					// peer is not on our chain
					// track back and try again
					self.imported_this_round = Some(0);
					return Err(BlockDownloaderImportError::Useless);
				} else {
					// TODO: validate heads better. E.g. check that there is enough distance between blocks.
					trace!(target: "sync", "Received {} subchain heads, proceeding to download", headers.len());
					self.blocks.reset_to(hashes);
					self.state = State::Blocks;
				}
			},
			State::Blocks => {
				let count = headers.len();
				self.blocks.insert_headers(headers);
				trace!(target: "sync", "Inserted {} headers", count);
			},
			_ => trace!(target: "sync", "Unexpected headers({})", headers.len()),
		}

		Ok(())
	}

	/// Called by peer once it has new block bodies
	pub fn import_bodies(&mut self, _io: &mut SyncIo, r: &UntrustedRlp) -> Result<(), BlockDownloaderImportError> {
		let item_count = r.item_count();
		if item_count == 0 {
			return Err(BlockDownloaderImportError::Useless);
		}
		else if self.state != State::Blocks {
			trace!(target: "sync", "Ignored unexpected block bodies");
		}
		else {
			let mut bodies = Vec::with_capacity(item_count);
			for i in 0..item_count {
				let body = try!(r.at(i).map_err(|e| {
					trace!(target: "sync", "Error decoding block boides RLP: {:?}", e);
					BlockDownloaderImportError::Invalid
				}));
				bodies.push(body.as_raw().to_vec());
			}
			if self.blocks.insert_bodies(bodies) != item_count {
				trace!(target: "sync", "Deactivating peer for giving invalid block bodies");
				return Err(BlockDownloaderImportError::Invalid);
			}
		}
		Ok(())
	}

	/// Called by peer once it has new block bodies
	pub fn import_receipts(&mut self, _io: &mut SyncIo, r: &UntrustedRlp) -> Result<(), BlockDownloaderImportError> {
		let item_count = r.item_count();
		if item_count == 0 {
			return Err(BlockDownloaderImportError::Useless);
		}
		else if self.state != State::Blocks {
			trace!(target: "sync", "Ignored unexpected block receipts");
		}
		else {
			let mut receipts = Vec::with_capacity(item_count);
			for i in 0..item_count {
				let receipt = try!(r.at(i).map_err(|e| {
					trace!(target: "sync", "Error decoding block receipts RLP: {:?}", e);
					BlockDownloaderImportError::Invalid
				}));
				receipts.push(receipt.as_raw().to_vec());
			}
			if self.blocks.insert_receipts(receipts) != item_count {
				trace!(target: "sync", "Deactivating peer for giving invalid block receipts");
				return Err(BlockDownloaderImportError::Invalid);
			}
		}
		Ok(())
	}

	fn start_sync_round(&mut self, io: &mut SyncIo) {
		self.state = State::ChainHead;
		trace!(target: "sync", "Starting round (last imported count = {:?}, block = {:?}", self.imported_this_round, self.last_imported_block);
		// Check if need to retract to find the common block. The problem is that the peers still return headers by hash even
		// from the non-canonical part of the tree. So we also retract if nothing has been imported last round.
		match self.imported_this_round {
			Some(n) if n == 0 && self.last_imported_block > 0 => {
				// nothing was imported last round, step back to a previous block
				// search parent in last round known parents first
				if let Some(&(_, p)) = self.round_parents.iter().find(|&&(h, _)| h == self.last_imported_hash) {
					self.last_imported_block -= 1;
					self.last_imported_hash = p.clone();
					trace!(target: "sync", "Searching common header from the last round {} ({})", self.last_imported_block, self.last_imported_hash);
				} else {
					match io.chain().block_hash(BlockID::Number(self.last_imported_block - 1)) {
						Some(h) => {
							self.last_imported_block -= 1;
							self.last_imported_hash = h;
							trace!(target: "sync", "Searching common header in the blockchain {} ({})", self.last_imported_block, self.last_imported_hash);
						}
						None => {
							debug!(target: "sync", "Could not revert to previous block, last: {} ({})", self.last_imported_block, self.last_imported_hash);
						}
					}
				}
			},
			_ => (),
		}
		self.imported_this_round = None;
	}

	/// Find some headers or blocks to download for a peer.
	pub fn request_blocks(&mut self, io: &mut SyncIo) -> Option<BlockRequest> {
		match self.state {
			State::Idle => {
				self.start_sync_round(io);
				return self.request_blocks(io);
			},
			State::ChainHead => {
				// Request subchain headers
				trace!(target: "sync", "Starting sync with better chain");
				// Request MAX_HEADERS_TO_REQUEST - 2 headers apart so that
				// MAX_HEADERS_TO_REQUEST would include headers for neighbouring subchains
				return Some(BlockRequest::Headers {
					start: self.last_imported_hash.clone(),
					count: SUBCHAIN_SIZE,
					skip: (MAX_HEADERS_TO_REQUEST - 2) as u64,
				});
			},
			State::Blocks => {
				// check to see if we need to download any block bodies first
				let needed_bodies = self.blocks.needed_bodies(MAX_BODIES_TO_REQUEST, false);
				if !needed_bodies.is_empty() {
					return Some(BlockRequest::Bodies {
						hashes: needed_bodies,
					});
				}

				if self.download_receipts {
					let needed_receipts = self.blocks.needed_receipts(MAX_RECEPITS_TO_REQUEST, false);
					if !needed_receipts.is_empty() {
						return Some(BlockRequest::Receipts {
							hashes: needed_receipts,
						});
					}
				}

				// find subchain to download
				if let Some((h, count)) = self.blocks.needed_headers(MAX_HEADERS_TO_REQUEST, false) {
					return Some(BlockRequest::Headers {
						start: h,
						count: count as u64,
						skip: 0,
					});
				}
			},
			State::Complete => (),
		}
		None
	}

	/// Checks if there are blocks fully downloaded that can be imported into the blockchain and does the import.
	pub fn collect_blocks(&mut self, io: &mut SyncIo, allow_out_of_order: bool) -> Result<(), BlockDownloaderImportError> {
		let mut bad = false;
		let mut imported = HashSet::new();
		let blocks = self.blocks.drain();
		let count = blocks.len();
		for block_and_receipts in blocks {
			let block = block_and_receipts.block;
			let receipts = block_and_receipts.receipts;
			let (h, number, parent) = {
				let header = BlockView::new(&block).header_view();
				(header.sha3(), header.number(), header.parent_hash())
			};

			// Perform basic block verification
			if !Block::is_good(&block) {
				debug!(target: "sync", "Bad block rlp {:?} : {:?}", h, block);
				bad = true;
				break;
			}

			if self.target_hash.as_ref().map_or(false, |t| t == &h) {
				self.state = State::Complete;
				trace!(target: "sync", "Sync target reached");
				return Ok(());
			}

			let result = if let Some(receipts) = receipts {
				io.chain().import_block_with_receipts(block, receipts)
			} else {
				io.chain().import_block(block)
			};

			match result {
				Err(BlockImportError::Import(ImportError::AlreadyInChain)) => {
					trace!(target: "sync", "Block already in chain {:?}", h);
					self.block_imported(&h, number, &parent);
				},
				Err(BlockImportError::Import(ImportError::AlreadyQueued)) => {
					trace!(target: "sync", "Block already queued {:?}", h);
					self.block_imported(&h, number, &parent);
				},
				Ok(_) => {
					trace!(target: "sync", "Block queued {:?}", h);
					imported.insert(h.clone());
					self.block_imported(&h, number, &parent);
				},
				Err(BlockImportError::Block(BlockError::UnknownParent(_))) if allow_out_of_order => {
					trace!(target: "sync", "Unknown new block parent, restarting sync");
					break;
				},
				Err(e) => {
					debug!(target: "sync", "Bad block {:?} : {:?}", h, e);
					bad = true;
					break;
				}
			}
		}
		trace!(target: "sync", "Imported {} of {}", imported.len(), count);
		self.imported_this_round = Some(self.imported_this_round.unwrap_or(0) + imported.len());

		if bad {
			return Err(BlockDownloaderImportError::Invalid);
		}

		if self.blocks.is_empty() {
			// complete sync round
			trace!(target: "sync", "Sync round complete");
			self.reset();
		}
		Ok(())
	}

	fn block_imported(&mut self, hash: &H256, number: BlockNumber, parent: &H256) {
		self.last_imported_block = number;
		self.last_imported_hash = hash.clone();
		self.round_parents.push_back((hash.clone(), parent.clone()));
		if self.round_parents.len() > MAX_ROUND_PARENTS {
			self.round_parents.pop_front();
		}
	}
}

#[cfg(test)]
mod tests {
}

