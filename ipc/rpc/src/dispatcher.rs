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

//! Async dispatcher

use futures::{Poll, Future, Task};
use std::collections::BTreeMap;
use util::RwLock;

struct Invoke {
	id: u64,
	method_num: u16,
	paylod: Vec<u8>,
}

struct InvokeResult {
	id: u64,
	payload: RwLock<Option<Vec<u8>>>,
}

struct InvokeFutureError;

impl Future for InvokeResult {
    type Item = Vec<u8>;
    type Error = InvokeFutureError;

    fn poll(&mut self, _task: &mut Task) -> Poll<Self::Item, Self::Error> {
		let mut payload = self.payload.write();
		match payload.take() {
			Some(bytes) => Poll::Ok(bytes),
			None => Poll::NotReady,
		}
	}

	fn schedule(&mut self, task: &mut Task) {
	}

}

struct Dispatcher {
	reg_counter: u64,
	dispatch_counter: u64,
	invokes: BTreeMap<u64, Invoke>,
	results: BTreeMap<u64, InvokeResult>,
}

