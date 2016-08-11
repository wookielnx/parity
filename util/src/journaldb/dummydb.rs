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

//! Memory-backed JournalDB implementation.
use super::JournalDB;
use memorydb::MemoryDB;
use hashdb::HashDB;
use error::UtilError;
use kvdb::{Database, DBTransaction};
use ::{Bytes, H256};

use std::collections::HashMap;
use std::sync::Arc;

/// Wraps two memory overlays: one with the canonical backing state,
/// and the other storing changes since it.
pub struct DummyDB {
	overlay: MemoryDB,
	backing: Arc<MemoryDB>,
}

impl DummyDB {
	/// Create a new DummyDB from a set of values to initialize the backing state with.
	pub fn new(items: &[Bytes]) -> Self {
		let mut backing = MemoryDB::new();
		for item in items {
			backing.insert(item);
		}

		DummyDB {
			overlay: MemoryDB::new(),
			backing: Arc::new(backing),
		}
	}
}

impl HashDB for DummyDB {
	fn keys(&self) -> HashMap<H256, i32> {
		let mut ret = HashMap::new();

		for (key, rc) in self.overlay.keys().into_iter().chain(self.backing.keys()) {
			*ret.entry(key).or_insert(0) += rc;
		}

		ret
	}

	fn get(&self, key: &H256) -> Option<&[u8]> {
		match self.overlay.get(key) {
			Some(val) => Some(val),
			None =>
				self.backing.get(key).map(|val| self.overlay.denote(key, val.into()).0),
		}
	}

	fn contains(&self, key: &H256) -> bool { self.get(key).is_some() }

	fn insert(&mut self, value: &[u8]) -> H256 {
		self.overlay.insert(value)
	}

	fn emplace(&mut self, key: H256, value: Bytes) {
		self.overlay.emplace(key, value);
	}

	fn remove(&mut self, key: &H256) {
		self.overlay.remove(key)
	}

	fn insert_aux(&mut self, hash: Vec<u8>, value: Vec<u8>) {
		self.overlay.insert_aux(hash, value);
	}

	fn get_aux(&self, hash: &[u8]) -> Option<Vec<u8>> {
		self.overlay.get_aux(hash)
	}

	fn remove_aux(&mut self, hash: &[u8]) {
		self.overlay.remove_aux(hash)
	}
}

impl JournalDB for DummyDB {
	fn boxed_clone(&self) -> Box<JournalDB> {
		Box::new(DummyDB {
			overlay: self.overlay.clone(),
			backing: self.backing.clone(),
		})
	}

	fn mem_used(&self) -> usize {
		self.overlay.mem_used() + self.backing.mem_used()
	}

	fn is_empty(&self) -> bool {
		false
	}

	fn latest_era(&self) -> Option<u64> {
		None
	}

	fn state(&self, _id: &H256) -> Option<Bytes> { None }

	fn commit(&mut self, _batch: &DBTransaction, _now: u64, _id: &H256, _end: Option<(u64, H256)>) -> Result<u32, UtilError> {
		unimplemented!()
	}

	fn inject(&mut self, _batch: &DBTransaction) -> Result<u32, UtilError> {
		unimplemented!()
	}

	fn merkle_proof(&self) -> Vec<Bytes> {
		self.overlay.merkle_proof()
	}

	fn backing(&self) -> &Arc<Database> {
		unimplemented!()
	}
}