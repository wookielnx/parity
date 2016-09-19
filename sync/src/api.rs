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

use std::sync::Arc;
use std::collections::HashMap;
use util::Bytes;
use network::{NetworkProtocolHandler, NetworkService, NetworkContext, PeerId,
	NetworkConfiguration as BasicNetworkConfiguration, NonReservedPeerMode, NetworkError};
use util::{U256, H256};
use io::{TimerToken};
use ethcore::client::{BlockChainClient, ChainNotify};
use ethcore::snapshot::SnapshotService;
use ethcore::header::BlockNumber;
use sync_io::NetSyncIo;
use chain::{ChainSync, SyncStatus};
use std::net::{SocketAddr, AddrParseError};
use ipc::{BinaryConvertable, BinaryConvertError, IpcConfig};
use std::str::FromStr;
use parking_lot::RwLock;

/// Ethereum sync protocol
pub const ETH_PROTOCOL: &'static str = "eth";

/// Sync configuration
#[derive(Debug, Clone, Copy)]
pub struct SyncConfig {
	/// Max blocks to download ahead
	pub max_download_ahead_blocks: usize,
	/// Network ID
	pub network_id: U256,
	/// Fork block to check
	pub fork_block: Option<(BlockNumber, H256)>,
	/// Enable snapshot sync
	pub warp_sync: bool,
}

impl Default for SyncConfig {
	fn default() -> SyncConfig {
		SyncConfig {
			max_download_ahead_blocks: 20000,
			network_id: U256::from(1),
			fork_block: None,
			warp_sync: true,
		}
	}
}

binary_fixed_size!(SyncConfig);
binary_fixed_size!(SyncStatus);

/// Current sync status
pub trait SyncProvider: Send + Sync {
	/// Get sync status
	fn status(&self) -> SyncStatus;
}

/// Ethereum network protocol handler
pub struct EthSync {
	/// Network service
	network: NetworkService,
	/// Protocol handler
	handler: Arc<SyncProtocolHandler>,
}

impl EthSync {
	/// Creates and register protocol with the network service
	pub fn new(config: SyncConfig, chain: Arc<BlockChainClient>, snapshot_service: Arc<SnapshotService>, network_config: NetworkConfiguration) -> Result<Arc<EthSync>, NetworkError> {
		let chain_sync = ChainSync::new(config, &*chain);
		let service = try!(NetworkService::new(try!(network_config.into_basic())));
		let sync = Arc::new(EthSync{
			network: service,
			handler: Arc::new(SyncProtocolHandler {
				sync: RwLock::new(chain_sync),
				chain: chain,
				snapshot_service: snapshot_service,
				overlay: RwLock::new(HashMap::new()),
			}),
		});

		Ok(sync)
	}
}

#[derive(Ipc)]
#[ipc(client_ident="SyncClient")]
impl SyncProvider for EthSync {
	/// Get sync status
	fn status(&self) -> SyncStatus {
		self.handler.sync.write().status()
	}
}

struct SyncProtocolHandler {
	/// Shared blockchain client.
	chain: Arc<BlockChainClient>,
	/// Shared snapshot service.
	snapshot_service: Arc<SnapshotService>,
	/// Sync strategy
	sync: RwLock<ChainSync>,
	/// Chain overlay used to cache data such as fork block.
	overlay: RwLock<HashMap<BlockNumber, Bytes>>,
}

impl NetworkProtocolHandler for SyncProtocolHandler {
	fn initialize(&self, io: &NetworkContext) {
		io.register_timer(0, 1000).expect("Error registering sync timer");
	}

	fn read(&self, io: &NetworkContext, peer: &PeerId, packet_id: u8, data: &[u8]) {
		ChainSync::dispatch_packet(&self.sync, &mut NetSyncIo::new(io, &*self.chain, &*self.snapshot_service, &self.overlay), *peer, packet_id, data);
	}

	fn connected(&self, io: &NetworkContext, peer: &PeerId) {
		self.sync.write().on_peer_connected(&mut NetSyncIo::new(io, &*self.chain, &*self.snapshot_service, &self.overlay), *peer);
	}

	fn disconnected(&self, io: &NetworkContext, peer: &PeerId) {
		self.sync.write().on_peer_aborting(&mut NetSyncIo::new(io, &*self.chain, &*self.snapshot_service, &self.overlay), *peer);
	}

	fn timeout(&self, io: &NetworkContext, _timer: TimerToken) {
		self.sync.write().maintain_peers(&mut NetSyncIo::new(io, &*self.chain, &*self.snapshot_service, &self.overlay));
		self.sync.write().maintain_sync(&mut NetSyncIo::new(io, &*self.chain, &*self.snapshot_service, &self.overlay));
		self.sync.write().propagate_new_transactions(&mut NetSyncIo::new(io, &*self.chain, &*self.snapshot_service, &self.overlay));
	}
}

impl ChainNotify for EthSync {
	fn new_blocks(&self,
		imported: Vec<H256>,
		invalid: Vec<H256>,
		enacted: Vec<H256>,
		retracted: Vec<H256>,
		sealed: Vec<H256>,
		_duration: u64)
	{
		self.network.with_context(ETH_PROTOCOL, |context| {
			let mut sync_io = NetSyncIo::new(context, &*self.handler.chain, &*self.handler.snapshot_service, &self.handler.overlay);
			self.handler.sync.write().chain_new_blocks(
				&mut sync_io,
				&imported,
				&invalid,
				&enacted,
				&retracted,
				&sealed);
		});
	}

	fn start(&self) {
		self.network.start().unwrap_or_else(|e| warn!("Error starting network: {:?}", e));
		self.network.register_protocol(self.handler.clone(), ETH_PROTOCOL, &[62u8, 63u8, 64u8])
			.unwrap_or_else(|e| warn!("Error registering ethereum protocol: {:?}", e));
	}

	fn stop(&self) {
		self.network.stop().unwrap_or_else(|e| warn!("Error stopping network: {:?}", e));
	}
}

impl IpcConfig for ManageNetwork { }
impl IpcConfig for SyncProvider { }

/// Trait for managing network
pub trait ManageNetwork : Send + Sync {
	/// Set to allow unreserved peers to connect
	fn accept_unreserved_peers(&self);
	/// Set to deny unreserved peers to connect
	fn deny_unreserved_peers(&self);
	/// Remove reservation for the peer
	fn remove_reserved_peer(&self, peer: String) -> Result<(), String>;
	/// Add reserved peer
	fn add_reserved_peer(&self, peer: String) -> Result<(), String>;
	/// Start network
	fn start_network(&self);
	/// Stop network
	fn stop_network(&self);
	/// Query the current configuration of the network
	fn network_config(&self) -> NetworkConfiguration;
}


#[derive(Ipc)]
#[ipc(client_ident="NetworkManagerClient")]
impl ManageNetwork for EthSync {
	fn accept_unreserved_peers(&self) {
		self.network.set_non_reserved_mode(NonReservedPeerMode::Accept);
	}

	fn deny_unreserved_peers(&self) {
		self.network.set_non_reserved_mode(NonReservedPeerMode::Deny);
	}

	fn remove_reserved_peer(&self, peer: String) -> Result<(), String> {
		self.network.remove_reserved_peer(&peer).map_err(|e| format!("{:?}", e))
	}

	fn add_reserved_peer(&self, peer: String) -> Result<(), String> {
		self.network.add_reserved_peer(&peer).map_err(|e| format!("{:?}", e))
	}

	fn start_network(&self) {
		self.start();
	}

	fn stop_network(&self) {
		self.network.with_context(ETH_PROTOCOL, |context| {
			let mut sync_io = NetSyncIo::new(context, &*self.handler.chain, &*self.handler.snapshot_service, &self.handler.overlay);
			self.handler.sync.write().abort(&mut sync_io);
		});
		self.stop();
	}

	fn network_config(&self) -> NetworkConfiguration {
		NetworkConfiguration::from(self.network.config().clone())
	}
}

#[derive(Binary, Debug, Clone, PartialEq, Eq)]
/// Network service configuration
pub struct NetworkConfiguration {
	/// Directory path to store general network configuration. None means nothing will be saved
	pub config_path: Option<String>,
	/// Directory path to store network-specific configuration. None means nothing will be saved
	pub net_config_path: Option<String>,
	/// IP address to listen for incoming connections. Listen to all connections by default
	pub listen_address: Option<String>,
	/// IP address to advertise. Detected automatically if none.
	pub public_address: Option<String>,
	/// Port for UDP connections, same as TCP by default
	pub udp_port: Option<u16>,
	/// Enable NAT configuration
	pub nat_enabled: bool,
	/// Enable discovery
	pub discovery_enabled: bool,
	/// List of initial node addresses
	pub boot_nodes: Vec<String>,
	/// Use provided node key instead of default
	pub use_secret: Option<H256>,
	/// Max number of connected peers to maintain
	pub max_peers: u32,
	/// Min number of connected peers to maintain
	pub min_peers: u32,
	/// List of reserved node addresses.
	pub reserved_nodes: Vec<String>,
	/// The non-reserved peer mode.
	pub allow_non_reserved: bool,
}

impl NetworkConfiguration {
	pub fn new() -> Self {
		From::from(BasicNetworkConfiguration::new())
	}

	pub fn new_local() -> Self {
		From::from(BasicNetworkConfiguration::new_local())
	}

	fn validate(&self) -> Result<(), AddrParseError> {
		if let Some(ref addr) = self.listen_address {
			try!(SocketAddr::from_str(&addr));
		}
		if let Some(ref addr) = self.public_address {
			try!(SocketAddr::from_str(&addr));
		}
		Ok(())
	}

	pub fn into_basic(self) -> Result<BasicNetworkConfiguration, AddrParseError> {

		Ok(BasicNetworkConfiguration {
			config_path: self.config_path,
			net_config_path: self.net_config_path,
			listen_address: match self.listen_address { None => None, Some(addr) => Some(try!(SocketAddr::from_str(&addr))) },
			public_address:  match self.public_address { None => None, Some(addr) => Some(try!(SocketAddr::from_str(&addr))) },
			udp_port: self.udp_port,
			nat_enabled: self.nat_enabled,
			discovery_enabled: self.discovery_enabled,
			boot_nodes: self.boot_nodes,
			use_secret: self.use_secret,
			max_peers: self.max_peers,
			min_peers: self.min_peers,
			reserved_nodes: self.reserved_nodes,
			non_reserved_mode: if self.allow_non_reserved { NonReservedPeerMode::Accept } else { NonReservedPeerMode::Deny },
		})
	}
}

impl From<BasicNetworkConfiguration> for NetworkConfiguration {
	fn from(other: BasicNetworkConfiguration) -> Self {
		NetworkConfiguration {
			config_path: other.config_path,
			net_config_path: other.net_config_path,
			listen_address: other.listen_address.and_then(|addr| Some(format!("{}", addr))),
			public_address: other.public_address.and_then(|addr| Some(format!("{}", addr))),
			udp_port: other.udp_port,
			nat_enabled: other.nat_enabled,
			discovery_enabled: other.discovery_enabled,
			boot_nodes: other.boot_nodes,
			use_secret: other.use_secret,
			max_peers: other.max_peers,
			min_peers: other.min_peers,
			reserved_nodes: other.reserved_nodes,
			allow_non_reserved: match other.non_reserved_mode { NonReservedPeerMode::Accept => true, _ => false } ,
		}
	}
}

#[derive(Debug, Binary, Clone)]
pub struct ServiceConfiguration {
	pub sync: SyncConfig,
	pub net: NetworkConfiguration,
	pub io_path: String,
}
