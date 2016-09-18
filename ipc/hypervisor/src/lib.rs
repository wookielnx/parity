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

//! Parity interprocess hypervisor module

#![cfg_attr(feature="dev", allow(used_underscore_binding))]

extern crate ethcore_ipc as ipc;
extern crate ethcore_ipc_nano as nanoipc;
extern crate semver;
#[macro_use] extern crate log;

pub mod service;

/// Default value for hypervisor ipc listener
pub const HYPERVISOR_IPC_URL: &'static str = "parity-internal-hyper-status.ipc";

use std::sync::{Arc,RwLock};
use service::{HypervisorService, IpcModuleId};
use std::process::{Command,Child};
use std::collections::HashMap;

pub use service::{ControlService, CLIENT_MODULE_ID, SYNC_MODULE_ID};

pub type BinaryId = &'static str;

pub struct Hypervisor {
	ipc_addr: String,
	service: Arc<HypervisorService>,
	processes: RwLock<HashMap<IpcModuleId, Child>>,
	modules: HashMap<IpcModuleId, BootArgs>,
	pub io_path: String,
}

/// Boot arguments for binary
pub struct BootArgs {
	cli: Option<Vec<String>>,
	stdin: Option<Vec<u8>>,
}

impl BootArgs {
	/// New empty boot arguments
	pub fn new() -> BootArgs {
		BootArgs {
			cli: None,
			stdin: None,
		}
	}

	/// Set command-line arguments for boot
	pub fn cli(mut self, cli: Vec<String>) -> BootArgs {
		self.cli = Some(cli);
		self
	}

	/// Set std-in stream for boot
	pub fn stdin(mut self, stdin: Vec<u8>) -> BootArgs {
		self.stdin = Some(stdin);
		self
	}
}

impl Hypervisor {
	/// initializes the Hypervisor service with the open ipc socket for incoming clients
	pub fn new() -> Hypervisor {
		Hypervisor::with_url(HYPERVISOR_IPC_URL)
	}

	pub fn module(mut self, module_id: IpcModuleId, args: BootArgs) -> Hypervisor {
		self.modules.insert(module_id, args);
		self.service.add_module(module_id);
		self
	}

	pub fn local_module(self, module_id: IpcModuleId) -> Hypervisor {
		self.service.add_module(module_id);
		self
	}

	pub fn io_path(mut self, directory: &str) -> Hypervisor {
		self.io_path = directory.to_owned();
		self
	}

	/// Starts with the specified address for the ipc listener and
	/// the specified list of modules in form of created service
	pub fn with_url(addr: &str) -> Hypervisor {
		unimplemented!()
	}

	/// Since one binary can host multiple modules
	/// we match binaries
	fn match_module(&self, module_id: &IpcModuleId) -> Option<&BootArgs> {
		self.modules.get(module_id)
	}

	/// Creates IPC listener and starts all binaries
	pub fn start(&self) {
	}

	/// Start binary for the specified module
	/// Does nothing when it is already started on module is inside the
	/// main binary
	fn start_module(&self, module_id: IpcModuleId) {
		use std::io::Write;

		self.match_module(&module_id).map(|boot_args| {
			let mut processes = self.processes.write().unwrap();
			{
				if processes.get(&module_id).is_some() {
					// already started for another module
					return;
				}
			}

			let mut command = Command::new(&std::env::current_exe().unwrap());
			command.stderr(std::process::Stdio::inherit());

			if let Some(ref cli_args) = boot_args.cli {
				for arg in cli_args { command.arg(arg); }
			}

			command.stdin(std::process::Stdio::piped());

			trace!(target: "hypervisor", "Spawn executable: {:?}", command);

			let mut child = command.spawn().unwrap_or_else(
				|e| panic!("Hypervisor cannot execute command ({:?}): {}", command, e));

			if let Some(ref std_in) = boot_args.stdin {
				trace!(target: "hypervisor", "Pushing std-in payload...");
				child.stdin.as_mut()
					.expect("std-in should be piped above")
					.write(std_in)
					.unwrap_or_else(|e| panic!(format!("Error trying to pipe stdin for {:?}: {:?}", &command, e)));
				drop(child.stdin.take());
			}

			processes.insert(module_id, child);
		});
	}

	/// Reports if all modules are checked in
	pub fn modules_ready(&self) -> bool {
		self.service.unchecked_count() == 0
	}

	pub fn modules_shutdown(&self) -> bool {
		self.service.running_count() == 0
	}

	/// Waits for every required module to check in
	pub fn wait_for_startup(&self) {
	}

	/// Waits for every required module to check in
	pub fn wait_for_shutdown(&self) {
	}

	/// Shutdown the ipc and all managed child processes
	pub fn shutdown(&self) {
		let mut childs = self.processes.write().unwrap();
		for (ref mut module, _) in childs.iter_mut() {
			trace!(target: "hypervisor", "Stopping process module: {}", module);
			self.service.send_shutdown(**module);
		}
		trace!(target: "hypervisor", "Waiting for shutdown...");
		self.wait_for_shutdown();
		trace!(target: "hypervisor", "All modules reported shutdown");
	}
}

impl Drop for Hypervisor {
	fn drop(&mut self) {
		self.shutdown();
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::atomic::{AtomicBool,Ordering};
	use std::sync::Arc;
	use nanoipc;

	#[test]
	fn can_init() {
		let url = "ipc:///tmp/test-parity-hypervisor-10.ipc";
		let test_module_id = 8080u64;

		let hypervisor = Hypervisor::with_url(url).local_module(test_module_id);
		assert_eq!(false, hypervisor.modules_ready());
	}
}
