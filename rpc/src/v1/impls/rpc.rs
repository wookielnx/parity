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

//! RPC generic methods implementation.
use std::collections::BTreeMap;
use jsonrpc_core::*;
use v1::traits::Rpc;
use v1::helpers::params::expect_no_params;

/// RPC generic methods implementation.
pub struct RpcClient {
	modules: BTreeMap<String, String>,
	valid_apis: Vec<String>,
}

impl RpcClient {
	/// Creates new `RpcClient`.
	pub fn new(modules: BTreeMap<String, String>) -> Self {
		// geth 1.3.6 fails upon receiving unknown api
		let valid_apis = vec!["web3", "eth", "net", "personal", "rpc"];

		RpcClient {
			modules: modules,
			valid_apis: valid_apis.into_iter().map(|x| x.to_owned()).collect(),
		}
	}
}

impl Rpc for RpcClient {
	fn rpc_modules(&self, params: Params) -> Result<Value, Error> {
		try!(expect_no_params(params));
		let modules = self.modules.iter()
			.fold(BTreeMap::new(), |mut map, (k, v)| {
				map.insert(k.to_owned(), Value::String(v.to_owned()));
				map
			});
		Ok(Value::Object(modules))
	}

	fn modules(&self, params: Params) -> Result<Value, Error> {
		try!(expect_no_params(params));
		let modules = self.modules.iter()
			.filter(|&(k, _v)| {
				self.valid_apis.contains(k)
			})
			.fold(BTreeMap::new(), |mut map, (k, v)| {
				map.insert(k.to_owned(), Value::String(v.to_owned()));
				map
			});
		Ok(Value::Object(modules))
	}
}
