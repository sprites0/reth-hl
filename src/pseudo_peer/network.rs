use super::service::{BlockHashCache, BlockPoller};
use crate::{
    chainspec::{parser::chain_value_parser, HlChainSpec},
    node::network::HlNetworkPrimitives,
    HlPrimitives,
};
use reth_network::{
    config::{rng_secret_key, SecretKey},
    NetworkConfig, NetworkManager, PeersConfig,
};
use reth_network_peers::TrustedPeer;
use reth_provider::test_utils::NoopProvider;
use std::{str::FromStr, sync::Arc};
use tokio::sync::mpsc;

pub struct NetworkBuilder {
    secret: SecretKey,
    peer_config: PeersConfig,
    boot_nodes: Vec<TrustedPeer>,
    discovery_port: u16,
    listener_port: u16,
}

impl Default for NetworkBuilder {
    fn default() -> Self {
        Self {
            secret: rng_secret_key(),
            peer_config: PeersConfig::default().with_max_outbound(1).with_max_inbound(1),
            boot_nodes: vec![],
            discovery_port: 0,
            listener_port: 0,
        }
    }
}

impl NetworkBuilder {
    pub fn with_secret(mut self, secret: SecretKey) -> Self {
        self.secret = secret;
        self
    }

    pub fn with_peer_config(mut self, peer_config: PeersConfig) -> Self {
        self.peer_config = peer_config;
        self
    }

    pub fn with_boot_nodes(mut self, boot_nodes: Vec<TrustedPeer>) -> Self {
        self.boot_nodes = boot_nodes;
        self
    }

    pub fn with_ports(mut self, discovery_port: u16, listener_port: u16) -> Self {
        self.discovery_port = discovery_port;
        self.listener_port = listener_port;
        self
    }

    pub async fn build<BS>(
        self,
        block_source: Arc<Box<dyn super::sources::BlockSource>>,
        blockhash_cache: BlockHashCache,
    ) -> eyre::Result<(NetworkManager<HlNetworkPrimitives>, mpsc::Sender<()>)> {
        let builder = NetworkConfig::<(), HlNetworkPrimitives>::builder(self.secret)
            .boot_nodes(self.boot_nodes)
            .peer_config(self.peer_config)
            .discovery_port(self.discovery_port)
            .listener_port(self.listener_port);

        let (block_poller, start_tx) = BlockPoller::new_suspended(block_source, blockhash_cache);
        let config = builder.block_import(Box::new(block_poller)).build(Arc::new(NoopProvider::<
            HlChainSpec,
            HlPrimitives,
        >::new(
            chain_value_parser("mainnet").unwrap(),
        )));

        let network = NetworkManager::new(config).await.map_err(|e| eyre::eyre!(e))?;
        Ok((network, start_tx))
    }
}

pub async fn create_network_manager<BS>(
    destination_peer: String,
    block_source: Arc<Box<dyn super::sources::BlockSource>>,
    blockhash_cache: BlockHashCache,
) -> eyre::Result<(NetworkManager<HlNetworkPrimitives>, mpsc::Sender<()>)> {
    NetworkBuilder::default()
        .with_boot_nodes(vec![TrustedPeer::from_str(&destination_peer).unwrap()])
        .build::<BS>(block_source, blockhash_cache)
        .await
}
