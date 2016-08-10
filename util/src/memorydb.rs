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

//! Reference-counted memory-based `HashDB` implementation.

use hash::*;
use bytes::*;
use rlp::*;
use sha3::*;
use hashdb::*;
use heapsize::*;
use std::mem;
use std::collections::hash_map::{HashMap, Entry};

const STATIC_NULL_RLP: (&'static [u8], i32) = (&[0x80; 1], 1);

/// MemoryDB items.
#[derive(PartialEq, Clone, Debug)]
pub struct Item {
	/// The value this item holds.
	pub value: Bytes,
	/// The reference count of this value.
	pub rc: i32,
	/// Whether this item was denoted, i.e. initially read from a backing database
	pub denoted: bool,
}

impl HeapSizeOf for Item {
	fn heap_size_of_children(&self) -> usize { self.value.heap_size_of_children() }
}

/// Reference-counted memory-based `HashDB` implementation.
///
/// Use `new()` to create a new database. Insert items with `insert()`, remove items
/// with `remove()`, check for existence with `containce()` and lookup a hash to derive
/// the data with `get()`. Clear with `clear()` and purge the portions of the data
/// that have no references with `purge()`.
///
/// # Example
/// ```rust
/// extern crate ethcore_util;
/// use ethcore_util::hashdb::*;
/// use ethcore_util::memorydb::*;
/// fn main() {
///   let mut m = MemoryDB::new();
///   let d = "Hello world!".as_bytes();
///
///   let k = m.insert(d);
///   assert!(m.contains(&k));
///   assert_eq!(m.get(&k).unwrap(), d);
///
///   m.insert(d);
///   assert!(m.contains(&k));
///
///   m.remove(&k);
///   assert!(m.contains(&k));
///
///   m.remove(&k);
///   assert!(!m.contains(&k));
///
///   m.remove(&k);
///   assert!(!m.contains(&k));
///
///   m.insert(d);
///   assert!(!m.contains(&k));

///   m.insert(d);
///   assert!(m.contains(&k));
///   assert_eq!(m.get(&k).unwrap(), d);
///
///   m.remove(&k);
///   assert!(!m.contains(&k));
/// }
/// ```
#[derive(Default, Clone, PartialEq)]
pub struct MemoryDB {
	data: H256FastMap<Item>,
	aux: HashMap<Bytes, Bytes>,
}

impl MemoryDB {
	/// Create a new instance of the memory DB.
	pub fn new() -> MemoryDB {
		MemoryDB {
			data: H256FastMap::default(),
			aux: HashMap::new(),
		}
	}

	/// Clear all data from the database.
	///
	/// # Examples
	/// ```rust
	/// extern crate ethcore_util;
	/// use ethcore_util::hashdb::*;
	/// use ethcore_util::memorydb::*;
	/// fn main() {
	///   let mut m = MemoryDB::new();
	///   let hello_bytes = "Hello world!".as_bytes();
	///   let hash = m.insert(hello_bytes);
	///   assert!(m.contains(&hash));
	///   m.clear();
	///   assert!(!m.contains(&hash));
	/// }
	/// ```
	pub fn clear(&mut self) {
		self.data.clear();
	}

	/// Purge all zero-referenced non-denoted data from the database.
	pub fn purge(&mut self) {
		let empties: Vec<_> = self.data.iter()
			.filter(|&(_, item)| item.rc == 0 && !item.denoted )
			.map(|(k, _)| k.clone())
			.collect();
		for empty in empties { self.data.remove(&empty); }
	}

	/// Return the internal map of hashes to data, clearing the current state.
	pub fn drain(&mut self) -> H256FastMap<Item> {
		mem::replace(&mut self.data, H256FastMap::default())
	}

	/// Return the internal map of auxiliary data, clearing the current state.
	pub fn drain_aux(&mut self) -> HashMap<Bytes, Bytes> {
		mem::replace(&mut self.aux, HashMap::new())
	}

	/// Grab the raw information associated with a key. Returns None if the key
	/// doesn't exist.
	///
	/// Even when Some is returned, the data is only guaranteed to be useful
	/// when the refs > 0.
	pub fn raw(&self, key: &H256) -> Option<(&[u8], i32)> {
		if key == &SHA3_NULL_RLP {
			return Some(STATIC_NULL_RLP.clone());
		}
		self.data.get(key).map(|ref item| (&item.value[..], item.rc))
	}

	/// Denote than an existing value has the given key. Used when a key gets removed without
	/// a prior insert and thus has a negative reference with no value.
	///
	/// May safely be called even if the key's value is known, in which case it will be a no-op.
	pub fn denote(&self, key: &H256, value: Bytes) -> (&[u8], i32) {
		if self.raw(key) == None {
			let item = Item {
				value: value,
				rc: 0,
				denoted: true,
			};

			unsafe {
				let p = &self.data as *const H256FastMap<Item> as *mut H256FastMap<Item>;
				(*p).insert(key.clone(), item);
			}
		}
		self.raw(key).unwrap()
	}

	/// Returns the size of allocated heap memory
	pub fn mem_used(&self) -> usize {
		self.data.heap_size_of_children()
		+ self.aux.heap_size_of_children()
	}

	/// Remove an element and delete it from storage if reference count reaches zero.
	pub fn remove_and_purge(&mut self, key: &H256) {
		if key == &SHA3_NULL_RLP {
			return;
		}
		match self.data.entry(key.clone()) {
			Entry::Occupied(mut entry) =>
				if entry.get().rc == 1 && !entry.get().denoted {
					entry.remove();
				} else {
					entry.get_mut().rc -= 1;
				},
			Entry::Vacant(entry) => {
				entry.insert(Item {
					value: Bytes::new(),
					rc: -1,
					denoted: false,
				});
			}
		}
	}

	/// Generate a merkle proof of the data this stores.
	/// This is based off of two assumptions:
	///  - any value with a negative reference count must exist in the backing database.
	///  - any value which is marked as denoted must have been read from the backing database.
	pub fn merkle_proof(&self) -> Vec<Bytes> {
		let mut v: Vec<Bytes> = Vec::new();
		for item in self.data.values() {
			if item.rc < 0 || item.denoted {
				// if the value is found, don't re-insert.
				if let Err(idx) = v.binary_search_by(|probe| probe.cmp(&item.value)) {
					v.insert(idx, item.value.clone());
				}
			}
		}
		v
	}
}

static NULL_RLP_STATIC: [u8; 1] = [0x80; 1];

impl HashDB for MemoryDB {
	fn get(&self, key: &H256) -> Option<&[u8]> {
		if key == &SHA3_NULL_RLP {
			return Some(&NULL_RLP_STATIC);
		}

		match self.data.get(key) {
			Some(ref item) if item.rc > 0 => Some(&item.value[..]),
			_ => None
		}
	}

	fn keys(&self) -> HashMap<H256, i32> {
		self.data.iter().filter_map(|(k, v)| if v.rc != 0 {Some((k.clone(), v.rc))} else {None}).collect()
	}

	fn contains(&self, key: &H256) -> bool {
		if key == &SHA3_NULL_RLP {
			return true;
		}

		match self.raw(key) {
			Some((_, x)) if x > 0 => true,
			_ => false
		}
	}

	fn insert(&mut self, value: &[u8]) -> H256 {
		if value == &NULL_RLP {
			return SHA3_NULL_RLP.clone();
		}

		let key = value.sha3();
		match self.data.entry(key) {
			Entry::Occupied(mut entry) => {
				let item = entry.get_mut();
				if item.rc <= 0 {
					item.value = value.into();
				}
				item.rc += 1;
			}
			Entry::Vacant(entry) => {
				entry.insert(Item {
					value: value.into(),
					rc: 1,
					denoted: false,
				});
			}
		}

		key
	}

	fn emplace(&mut self, key: H256, value: Bytes) {
		if value == &NULL_RLP {
			return;
		}

		match self.data.entry(key) {
			Entry::Occupied(mut entry) => {
				let item = entry.get_mut();
				if item.rc <= 0 {
					item.value = value.into();
				}
				item.rc += 1;
			}
			Entry::Vacant(entry) => {
				entry.insert(Item {
					value: value.into(),
					rc: 1,
					denoted: false,
				});
			}
		}
	}

	fn remove(&mut self, key: &H256) {
		if key == &SHA3_NULL_RLP {
			return;
		}

		match self.data.entry(key.clone()) {
			Entry::Occupied(mut entry) => {
				let item = entry.get_mut();
				item.rc -= 1;
			}
			Entry::Vacant(entry) => {
				entry.insert(Item {
					value: Bytes::new(),
					rc: -1,
					denoted: false,
				});
			}
		}
	}

	fn insert_aux(&mut self, hash: Vec<u8>, value: Vec<u8>) {
		self.aux.insert(hash, value);
	}

	fn get_aux(&self, hash: &[u8]) -> Option<Vec<u8>> {
		self.aux.get(hash).cloned()
	}

	fn remove_aux(&mut self, hash: &[u8]) {
		self.aux.remove(hash);
	}
}

#[test]
fn memorydb_denote() {
	let mut m = MemoryDB::new();
	let hello_bytes = b"Hello world!";
	let hash = m.insert(hello_bytes);
	assert_eq!(m.get(&hash).unwrap(), b"Hello world!");

	for _ in 0..1000 {
		let r = H256::random();
		let k = r.sha3();
		let (v, rc) = m.denote(&k, r.to_bytes());
		assert_eq!(v, r.as_slice());
		assert_eq!(rc, 0);
	}

	assert_eq!(m.get(&hash).unwrap(), b"Hello world!");
}

#[test]
fn memorydb_remove_and_purge() {
	let hello_bytes = b"Hello world!";
	let hello_key = hello_bytes.sha3();

	let mut m = MemoryDB::new();
	m.remove(&hello_key);
	assert_eq!(m.raw(&hello_key).unwrap().1, -1);
	m.purge();
	assert_eq!(m.raw(&hello_key).unwrap().1, -1);
	m.insert(hello_bytes);
	assert_eq!(m.raw(&hello_key).unwrap().1, 0);
	m.purge();
	assert_eq!(m.raw(&hello_key), None);

	let mut m = MemoryDB::new();
	m.remove_and_purge(&hello_key);
	assert_eq!(m.raw(&hello_key).unwrap().1, -1);
	m.insert(hello_bytes);
	m.insert(hello_bytes);
	assert_eq!(m.raw(&hello_key).unwrap().1, 1);
	m.remove_and_purge(&hello_key);
	assert_eq!(m.raw(&hello_key), None);
}
