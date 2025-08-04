#![allow(clippy::owned_cow)]
use crate::{
    consensus::HlConsensus,
    node::{
        network::block_import::{handle::ImportHandle, service::ImportService, HlBlockImport},
        primitives::HlPrimitives,
        rpc::engine_api::payload::HlPayloadTypes,
        types::ReadPrecompileCalls,
        HlNode,
    },
    pseudo_peer::{start_pseudo_peer, BlockSourceConfig},
    HlBlock,
};
use alloy_rlp::{Decodable, Encodable};
// use handshake::HlHandshake;
use reth::{
    api::{FullNodeTypes, TxTy},
    builder::{components::NetworkBuilder, BuilderContext},
    transaction_pool::{PoolTransaction, TransactionPool},
};
use reth_discv4::NodeRecord;
use reth_engine_primitives::BeaconConsensusEngineHandle;
use reth_eth_wire::{BasicNetworkPrimitives, NewBlock, NewBlockPayload};
use reth_ethereum_primitives::PooledTransactionVariant;
use reth_network::{NetworkConfig, NetworkHandle, NetworkManager};
use reth_network_api::PeersInfo;
use reth_provider::StageCheckpointReader;
use reth_stages_types::StageId;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::info;

pub mod block_import;
// pub mod handshake;
// pub(crate) mod upgrade_status;
/// HL `NewBlock` message value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HlNewBlock(pub NewBlock<HlBlock>);

mod rlp {
    use super::*;
    use crate::{
        node::primitives::{BlockBody, TransactionSigned},
        HlBlockBody,
    };
    use alloy_consensus::{BlobTransactionSidecar, Header};
    use alloy_primitives::{Address, U128};
    use alloy_rlp::{RlpDecodable, RlpEncodable};
    use alloy_rpc_types::Withdrawals;
    use std::borrow::Cow;

    #[derive(RlpEncodable, RlpDecodable)]
    #[rlp(trailing)]
    struct BlockHelper<'a> {
        header: Cow<'a, Header>,
        transactions: Cow<'a, Vec<TransactionSigned>>,
        ommers: Cow<'a, Vec<Header>>,
        withdrawals: Option<Cow<'a, Withdrawals>>,
    }

    #[derive(RlpEncodable, RlpDecodable)]
    #[rlp(trailing)]
    struct HlNewBlockHelper<'a> {
        block: BlockHelper<'a>,
        td: U128,
        sidecars: Option<Cow<'a, Vec<BlobTransactionSidecar>>>,
        read_precompile_calls: Option<Cow<'a, ReadPrecompileCalls>>,
        highest_precompile_address: Option<Cow<'a, Address>>,
    }

    impl<'a> From<&'a HlNewBlock> for HlNewBlockHelper<'a> {
        fn from(value: &'a HlNewBlock) -> Self {
            let HlNewBlock(NewBlock {
                block:
                    HlBlock {
                        header,
                        body:
                            HlBlockBody {
                                inner: BlockBody { transactions, ommers, withdrawals },
                                sidecars,
                                read_precompile_calls,
                                highest_precompile_address,
                            },
                    },
                td,
            }) = value;

            Self {
                block: BlockHelper {
                    header: Cow::Borrowed(header),
                    transactions: Cow::Borrowed(transactions),
                    ommers: Cow::Borrowed(ommers),
                    withdrawals: withdrawals.as_ref().map(Cow::Borrowed),
                },
                td: *td,
                sidecars: sidecars.as_ref().map(Cow::Borrowed),
                read_precompile_calls: read_precompile_calls.as_ref().map(Cow::Borrowed),
                highest_precompile_address: highest_precompile_address.as_ref().map(Cow::Borrowed),
            }
        }
    }

    impl Encodable for HlNewBlock {
        fn encode(&self, out: &mut dyn bytes::BufMut) {
            HlNewBlockHelper::from(self).encode(out);
        }

        fn length(&self) -> usize {
            HlNewBlockHelper::from(self).length()
        }
    }

    impl Decodable for HlNewBlock {
        fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
            let HlNewBlockHelper {
                block: BlockHelper { header, transactions, ommers, withdrawals },
                td,
                sidecars,
                read_precompile_calls,
                highest_precompile_address,
            } = HlNewBlockHelper::decode(buf)?;

            Ok(HlNewBlock(NewBlock {
                block: HlBlock {
                    header: header.into_owned(),
                    body: HlBlockBody {
                        inner: BlockBody {
                            transactions: transactions.into_owned(),
                            ommers: ommers.into_owned(),
                            withdrawals: withdrawals.map(|w| w.into_owned()),
                        },
                        sidecars: sidecars.map(|s| s.into_owned()),
                        read_precompile_calls: read_precompile_calls.map(|s| s.into_owned()),
                        highest_precompile_address: highest_precompile_address
                            .map(|s| s.into_owned()),
                    },
                },
                td,
            }))
        }
    }
}

impl NewBlockPayload for HlNewBlock {
    type Block = HlBlock;

    fn block(&self) -> &Self::Block {
        &self.0.block
    }
}

/// Network primitives for HL.
pub type HlNetworkPrimitives =
    BasicNetworkPrimitives<HlPrimitives, PooledTransactionVariant, HlNewBlock>;

/// A basic hl network builder.
#[derive(Debug)]
pub struct HlNetworkBuilder {
    pub(crate) engine_handle_rx:
        Arc<Mutex<Option<oneshot::Receiver<BeaconConsensusEngineHandle<HlPayloadTypes>>>>>,

    pub(crate) block_source_config: BlockSourceConfig,
}

impl HlNetworkBuilder {
    /// Returns the [`NetworkConfig`] that contains the settings to launch the p2p network.
    ///
    /// This applies the configured [`HlNetworkBuilder`] settings.
    pub fn network_config<Node>(
        self,
        ctx: &BuilderContext<Node>,
    ) -> eyre::Result<NetworkConfig<Node::Provider, HlNetworkPrimitives>>
    where
        Node: FullNodeTypes<Types = HlNode>,
    {
        let Self { engine_handle_rx, .. } = self;

        let network_builder = ctx.network_config_builder()?;

        let (to_import, from_network) = mpsc::unbounded_channel();
        let (to_network, import_outcome) = mpsc::unbounded_channel();

        let handle = ImportHandle::new(to_import, import_outcome);
        let consensus = Arc::new(HlConsensus { provider: ctx.provider().clone() });

        ctx.task_executor().spawn_critical("block import", async move {
            let handle = engine_handle_rx
                .lock()
                .await
                .take()
                .expect("node should only be launched once")
                .await
                .unwrap();

            ImportService::new(consensus, handle, from_network, to_network).await.unwrap();
        });

        let network_builder = network_builder
            .disable_dns_discovery()
            .disable_nat()
            .boot_nodes(boot_nodes())
            .set_head(ctx.head())
            .with_pow()
            .block_import(Box::new(HlBlockImport::new(handle)));
        // .discovery(discv4)
        // .eth_rlpx_handshake(Arc::new(HlHandshake::default()));

        let network_config = ctx.build_network_config(network_builder);

        Ok(network_config)
    }
}

impl<Node, Pool> NetworkBuilder<Node, Pool> for HlNetworkBuilder
where
    Node: FullNodeTypes<Types = HlNode>,
    Pool: TransactionPool<
            Transaction: PoolTransaction<
                Consensus = TxTy<Node::Types>,
                Pooled = PooledTransactionVariant,
            >,
        > + Unpin
        + 'static,
{
    type Network = NetworkHandle<HlNetworkPrimitives>;

    async fn build_network(
        self,
        ctx: &BuilderContext<Node>,
        pool: Pool,
    ) -> eyre::Result<Self::Network> {
        let block_source_config = self.block_source_config.clone();
        let network_config = self.network_config(ctx)?;
        let network = NetworkManager::builder(network_config).await?;
        let handle = ctx.start_network(network, pool);
        let local_node_record = handle.local_node_record();
        let chain_spec = ctx.chain_spec();
        info!(target: "reth::cli", enode=%local_node_record, "P2P networking initialized");

        let next_block_number =
            ctx.provider().get_stage_checkpoint(StageId::Finish)?.unwrap_or_default().block_number
                + 1;

        ctx.task_executor().spawn_critical("pseudo peer", async move {
            let block_source =
                block_source_config.create_cached_block_source(next_block_number).await;
            start_pseudo_peer(chain_spec, local_node_record.to_string(), block_source)
                .await
                .unwrap();
        });

        Ok(handle)
    }
}

/// HL mainnet bootnodes <https://github.com/bnb-chain/hl/blob/master/params/bootnodes.go#L23>
static BOOTNODES: [&str; 0] = [];

pub fn boot_nodes() -> Vec<NodeRecord> {
    BOOTNODES[..].iter().map(|s| s.parse().unwrap()).collect()
}
