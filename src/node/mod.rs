use crate::{
    chainspec::HlChainSpec,
    node::{
        primitives::{HlBlock, HlBlockBody, HlPrimitives},
        rpc::{
            engine_api::{
                builder::HlEngineApiBuilder, payload::HlPayloadTypes,
                validator::HlEngineValidatorBuilder,
            },
            HlEthApiBuilder,
        },
        storage::HlStorage,
    },
};
use consensus::HlConsensusBuilder;
use engine::HlPayloadServiceBuilder;
use evm::HlExecutorBuilder;
use network::HlNetworkBuilder;
use reth::{
    api::{FullNodeComponents, FullNodeTypes, NodeTypes},
    builder::{
        components::ComponentsBuilder, rpc::RpcAddOns, DebugNode, Node, NodeAdapter,
        NodeComponentsBuilder,
    },
};
use reth_engine_primitives::BeaconConsensusEngineHandle;
use reth_node_ethereum::node::EthereumPoolBuilder;
use reth_primitives::BlockBody;
use reth_trie_db::MerklePatriciaTrie;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};

pub mod cli;
pub mod consensus;
pub mod engine;
pub mod evm;
pub mod network;
pub mod primitives;
pub mod rpc;
pub mod spot_meta;
pub mod storage;
pub mod types;

/// Hl addons configuring RPC types
pub type HlNodeAddOns<N> =
    RpcAddOns<N, HlEthApiBuilder, HlEngineValidatorBuilder, HlEngineApiBuilder>;

/// Type configuration for a regular Hl node.
#[derive(Debug, Clone)]
pub struct HlNode {
    engine_handle_rx:
        Arc<Mutex<Option<oneshot::Receiver<BeaconConsensusEngineHandle<HlPayloadTypes>>>>>,
}

impl HlNode {
    pub fn new() -> (Self, oneshot::Sender<BeaconConsensusEngineHandle<HlPayloadTypes>>) {
        let (tx, rx) = oneshot::channel();
        (Self { engine_handle_rx: Arc::new(Mutex::new(Some(rx))) }, tx)
    }
}

impl HlNode {
    pub fn components<Node>(
        &self,
    ) -> ComponentsBuilder<
        Node,
        EthereumPoolBuilder,
        HlPayloadServiceBuilder,
        HlNetworkBuilder,
        HlExecutorBuilder,
        HlConsensusBuilder,
    >
    where
        Node: FullNodeTypes<Types = Self>,
    {
        ComponentsBuilder::default()
            .node_types::<Node>()
            .pool(EthereumPoolBuilder::default())
            .executor(HlExecutorBuilder::default())
            .payload(HlPayloadServiceBuilder::default())
            .network(HlNetworkBuilder { engine_handle_rx: self.engine_handle_rx.clone() })
            .consensus(HlConsensusBuilder::default())
    }
}

impl NodeTypes for HlNode {
    type Primitives = HlPrimitives;
    type ChainSpec = HlChainSpec;
    type StateCommitment = MerklePatriciaTrie;
    type Storage = HlStorage;
    type Payload = HlPayloadTypes;
}

impl<N> Node<N> for HlNode
where
    N: FullNodeTypes<Types = Self>,
{
    type ComponentsBuilder = ComponentsBuilder<
        N,
        EthereumPoolBuilder,
        HlPayloadServiceBuilder,
        HlNetworkBuilder,
        HlExecutorBuilder,
        HlConsensusBuilder,
    >;

    type AddOns = HlNodeAddOns<
        NodeAdapter<N, <Self::ComponentsBuilder as NodeComponentsBuilder<N>>::Components>,
    >;

    fn components_builder(&self) -> Self::ComponentsBuilder {
        Self::components(self)
    }

    fn add_ons(&self) -> Self::AddOns {
        HlNodeAddOns::default()
    }
}

impl<N> DebugNode<N> for HlNode
where
    N: FullNodeComponents<Types = Self>,
{
    type RpcBlock = alloy_rpc_types::Block;

    fn rpc_to_primitive_block(rpc_block: Self::RpcBlock) -> HlBlock {
        let alloy_rpc_types::Block { header, transactions, withdrawals, .. } = rpc_block;
        HlBlock {
            header: header.inner,
            body: HlBlockBody {
                inner: BlockBody {
                    transactions: transactions
                        .into_transactions()
                        .map(|tx| tx.inner.into_inner().into())
                        .collect(),
                    ommers: Default::default(),
                    withdrawals,
                },
                sidecars: None,
                read_precompile_calls: None,
            },
        }
    }
}
