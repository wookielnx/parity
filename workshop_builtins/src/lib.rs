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

//! Built-in contract implementations for the RustFest 2016 blockchain workshop.
//!
//!

extern crate ethcore;
extern crate ethcore_util as util;

// These are the types needed for built-in smart contract implementation.
//
// The `Builtin` type consists of two fields:
//   - the implementation, an `Arc<BuiltinImpl>` which accepts
//     an `&[u8]` as input and an `&mut [u8]` as output. Output should be truncated
//     if it won't fit into the output slice.
//
//   - the pricing scheme, an `Arc<Pricer>`,
//     which indicates the "gas" cost per byte of input data. The "gas" is how much
//     value an invocation of this builtin will cost, in increments of 1e-18 Ether.
//     Since these increments are so small, we use a 256-bit unsigned integer to
//     hold them.
//
//     One such pricing scheme is the `ethcore::builtin::Linear` scheme, which
//     consists of a base cost and a cost per 8-byte word, rounded up.
//
//     Import it if you'd find that useful!
use ethcore::builtin::{Builtin, Pricer, Impl as BuiltinImpl};

// An address is a 160-bit hash, which can be used to uniquely refer
// to an account. This implements `From<u64>` and `From<&'static str>` for convenience.
use util::Address;

// This is a 256-bit unsigned integer type. We use these whenever we have to list a cost, value,
// or other amount of Ether so that we can specify really tiny amounts.
use util::U256;

/// This function will be called by `parity` on startup to produce a set of
/// builtins to register to the given addresses. Note that the addresses
/// `0000000000000000000000000000000000000001`,
/// `0000000000000000000000000000000000000002`,
/// `0000000000000000000000000000000000000003`,
/// and `0000000000000000000000000000000000000004`
/// are already registered to some special builtins, so try to
/// avoid overwriting those -- if you do, things might not work quite as
/// expected.
///
/// Once that's done, you'll be able to interact with each of these from
/// other smart contracts by issuing a call to their address.
/// We'll show you how to do that once you're ready.
pub fn produce_builtins() -> Vec<(Address, Builtin)> {
	// Nothing here yet! Go out and write some builtins (or one really cool one!)
	vec![]
}

// Here's a sample pricer which only applies a base cost.
pub struct Flat(U256);

impl Pricer for Flat {
	fn cost(&self, _input_size: usize) -> U256 { self.0.clone() }
}

// And here's a sample implementation which will xor every byte with
// its index in the input array, mod 256.
pub struct LenXor;

impl BuiltinImpl for LenXor {
	fn execute(&self, input: &[u8], output: &mut [u8]) {
		for (idx, (x, y)) in input.iter().zip(output).enumerate() {
			*y = *x ^ (idx as u8)
		}
	}
}

// You can assemble any combination of pricing scheme and implementation into
// a builtin. Here's an example using the two we just made:
//
// 	let my_builtin = Builtin {
// 		pricer: Arc::new(Flat(100_000.into())),
// 		native: Arc::new(LenXor),
// 	};