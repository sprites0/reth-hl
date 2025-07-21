/// We need to override the following methods:
/// Filter:
/// - eth_getLogs
/// - eth_subscribe
///
/// Block (handled by HlEthApi already):
/// - eth_getBlockByNumber/eth_getBlockByHash
/// - eth_getBlockReceipts
use crate::HlBlock;
use alloy_rpc_types::{
    pubsub::{Params, SubscriptionKind},
    Filter, FilterChanges, FilterId, Log, PendingTransactionFilterKind,
};
use jsonrpsee::{PendingSubscriptionSink, SubscriptionMessage, SubscriptionSink};
use jsonrpsee_core::{async_trait, RpcResult};
use jsonrpsee_types::ErrorObject;
use reth::{
    api::FullNodeComponents, builder::rpc::RpcContext, rpc::result::internal_rpc_err,
    tasks::TaskSpawner,
};
use reth_network::NetworkInfo;
use reth_primitives::NodePrimitives;
use reth_primitives_traits::SignedTransaction as _;
use reth_provider::{BlockIdReader, BlockReader, TransactionsProvider};
use reth_rpc::{EthFilter, EthPubSub};
use reth_rpc_eth_api::{
    EthApiServer, EthFilterApiServer, EthPubSubApiServer, FullEthApiTypes, RpcBlock, RpcHeader,
    RpcNodeCore, RpcNodeCoreExt, RpcReceipt, RpcTransaction, RpcTxReq,
};
use serde::Serialize;
use std::{collections::HashSet, sync::Arc};
use tokio_stream::{Stream, StreamExt};
use tracing::{info, trace};

pub trait EthWrapper:
    EthApiServer<
        RpcTxReq<Self::NetworkTypes>,
        RpcTransaction<Self::NetworkTypes>,
        RpcBlock<Self::NetworkTypes>,
        RpcReceipt<Self::NetworkTypes>,
        RpcHeader<Self::NetworkTypes>,
    > + FullEthApiTypes
    + RpcNodeCoreExt<
        Provider: BlockIdReader + BlockReader<Block = HlBlock>,
        Primitives: NodePrimitives<
            SignedTx = <<Self as RpcNodeCore>::Provider as TransactionsProvider>::Transaction,
        >,
        Network: NetworkInfo,
    > + 'static
{
}

impl <
    T:
        EthApiServer<
            RpcTxReq<Self::NetworkTypes>,
            RpcTransaction<Self::NetworkTypes>,
            RpcBlock<Self::NetworkTypes>,
            RpcReceipt<Self::NetworkTypes>,
            RpcHeader<Self::NetworkTypes>,
        > + FullEthApiTypes
        + RpcNodeCoreExt<
            Provider: BlockIdReader + BlockReader<Block = HlBlock>,
            Primitives: NodePrimitives<
                SignedTx = <<Self as RpcNodeCore>::Provider as TransactionsProvider>::Transaction,
            >,
            Network: NetworkInfo,
        > + 'static
> EthWrapper for T {
}

pub struct HlNodeFilterHttp<Eth: EthWrapper> {
    filter: Arc<EthFilter<Eth>>,
    provider: Arc<Eth::Provider>,
}

impl<Eth: EthWrapper> HlNodeFilterHttp<Eth> {
    pub fn new(filter: Arc<EthFilter<Eth>>, provider: Arc<Eth::Provider>) -> Self {
        Self { filter, provider }
    }
}

#[async_trait]
impl<Eth: EthWrapper> EthFilterApiServer<RpcTransaction<Eth::NetworkTypes>>
    for HlNodeFilterHttp<Eth>
{
    /// Handler for `eth_newFilter`
    async fn new_filter(&self, filter: Filter) -> RpcResult<FilterId> {
        trace!(target: "rpc::eth", "Serving eth_newFilter");
        self.filter.new_filter(filter).await
    }

    /// Handler for `eth_newBlockFilter`
    async fn new_block_filter(&self) -> RpcResult<FilterId> {
        trace!(target: "rpc::eth", "Serving eth_newBlockFilter");
        self.filter.new_block_filter().await
    }

    /// Handler for `eth_newPendingTransactionFilter`
    async fn new_pending_transaction_filter(
        &self,
        kind: Option<PendingTransactionFilterKind>,
    ) -> RpcResult<FilterId> {
        trace!(target: "rpc::eth", "Serving eth_newPendingTransactionFilter");
        self.filter.new_pending_transaction_filter(kind).await
    }

    /// Handler for `eth_getFilterChanges`
    async fn filter_changes(
        &self,
        id: FilterId,
    ) -> RpcResult<FilterChanges<RpcTransaction<Eth::NetworkTypes>>> {
        trace!(target: "rpc::eth", "Serving eth_getFilterChanges");
        self.filter.filter_changes(id).await.map_err(ErrorObject::from)
    }

    /// Returns an array of all logs matching filter with given id.
    ///
    /// Returns an error if no matching log filter exists.
    ///
    /// Handler for `eth_getFilterLogs`
    async fn filter_logs(&self, id: FilterId) -> RpcResult<Vec<Log>> {
        trace!(target: "rpc::eth", "Serving eth_getFilterLogs");
        self.filter.filter_logs(id).await.map_err(ErrorObject::from)
    }

    /// Handler for `eth_uninstallFilter`
    async fn uninstall_filter(&self, id: FilterId) -> RpcResult<bool> {
        trace!(target: "rpc::eth", "Serving eth_uninstallFilter");
        self.filter.uninstall_filter(id).await
    }

    /// Returns logs matching given filter object.
    ///
    /// Handler for `eth_getLogs`
    async fn logs(&self, filter: Filter) -> RpcResult<Vec<Log>> {
        trace!(target: "rpc::eth", "Serving eth_getLogs");
        let mut logs = self.filter.logs(filter).await?;

        let block_numbers: HashSet<_> = logs.iter().map(|log| log.block_number.unwrap()).collect();
        info!("block_numbers: {:?}", block_numbers);
        let system_tx_hashes: HashSet<_> = block_numbers
            .into_iter()
            .flat_map(|block_number| {
                let block = self.provider.block_by_number(block_number).unwrap().unwrap();
                let transactions = block.body.transactions().collect::<Vec<_>>();
                transactions
                    .iter()
                    .filter(|tx| tx.is_system_transaction())
                    .map(|tx| *tx.tx_hash())
                    .collect::<Vec<_>>()
            })
            .collect();

        logs.retain(|log| !system_tx_hashes.contains(&log.transaction_hash.unwrap()));
        Ok(logs)
    }
}

pub struct HlNodeFilterWs<Eth: EthWrapper> {
    pubsub: Arc<EthPubSub<Eth>>,
    provider: Arc<Eth::Provider>,
    subscription_task_spawner: Box<dyn TaskSpawner + 'static>,
}

impl<Eth: EthWrapper> HlNodeFilterWs<Eth> {
    pub fn new(
        pubsub: Arc<EthPubSub<Eth>>,
        provider: Arc<Eth::Provider>,
        subscription_task_spawner: Box<dyn TaskSpawner + 'static>,
    ) -> Self {
        Self { pubsub, provider, subscription_task_spawner }
    }
}

#[async_trait]
impl<Eth: EthWrapper> EthPubSubApiServer<RpcTransaction<Eth::NetworkTypes>>
    for HlNodeFilterWs<Eth>
{
    /// Handler for `eth_subscribe`
    async fn subscribe(
        &self,
        pending: PendingSubscriptionSink,
        kind: SubscriptionKind,
        params: Option<Params>,
    ) -> jsonrpsee::core::SubscriptionResult {
        let sink = pending.accept().await?;
        let pubsub = self.pubsub.clone();
        let provider = self.provider.clone();
        self.subscription_task_spawner.spawn(Box::pin(async move {
            if kind == SubscriptionKind::Logs {
                // if no params are provided, used default filter params
                let filter = match params {
                    Some(Params::Logs(filter)) => *filter,
                    Some(Params::Bool(_)) => {
                        return;
                    }
                    _ => Default::default(),
                };
                let _ = pipe_from_stream(
                    sink,
                    pubsub
                        .log_stream(filter)
                        .filter(|log| not_from_system_tx::<Eth>(log, &provider)),
                )
                .await;
            } else {
                let _ = pubsub.handle_accepted(sink, kind, params).await;
            };
        }));

        Ok(())
    }
}

fn not_from_system_tx<Eth: EthWrapper>(log: &Log, provider: &Eth::Provider) -> bool {
    let block = provider.block_by_number(log.block_number.unwrap()).unwrap().unwrap();
    let transactions = block.body.transactions().collect::<Vec<_>>();
    !transactions
        .iter()
        .filter(|tx| tx.is_system_transaction())
        .map(|tx| *tx.tx_hash())
        .any(|tx_hash| tx_hash == log.transaction_hash.unwrap())
}

/// Helper to convert a serde error into an [`ErrorObject`]
#[derive(Debug, thiserror::Error)]
#[error("Failed to serialize subscription item: {0}")]
pub struct SubscriptionSerializeError(#[from] serde_json::Error);

impl SubscriptionSerializeError {
    const fn new(err: serde_json::Error) -> Self {
        Self(err)
    }
}

impl From<SubscriptionSerializeError> for ErrorObject<'static> {
    fn from(value: SubscriptionSerializeError) -> Self {
        internal_rpc_err(value.to_string())
    }
}

async fn pipe_from_stream<T, St>(
    sink: SubscriptionSink,
    mut stream: St,
) -> Result<(), ErrorObject<'static>>
where
    St: Stream<Item = T> + Unpin,
    T: Serialize,
{
    loop {
        tokio::select! {
            _ = sink.closed() => {
                // connection dropped
                break Ok(())
            },
            maybe_item = stream.next() => {
                let item = match maybe_item {
                    Some(item) => item,
                    None => {
                        // stream ended
                        break  Ok(())
                    },
                };
                let msg = SubscriptionMessage::new(
                    sink.method_name(),
                    sink.subscription_id(),
                    &item
                ).map_err(SubscriptionSerializeError::new)?;

                if sink.send(msg).await.is_err() {
                    break Ok(());
                }
            }
        }
    }
}

pub fn install_hl_node_compliance<Node, EthApi>(
    ctx: RpcContext<Node, EthApi>,
) -> Result<(), eyre::Error>
where
    Node: FullNodeComponents,
    Node::Provider: BlockIdReader + BlockReader<Block = crate::HlBlock>,
    EthApi: EthWrapper,
{
    ctx.modules.replace_configured(
        HlNodeFilterHttp::new(
            Arc::new(ctx.registry.eth_handlers().filter.clone()),
            Arc::new(ctx.registry.eth_api().provider().clone()),
        )
        .into_rpc(),
    )?;
    ctx.modules.replace_configured(
        HlNodeFilterWs::new(
            Arc::new(ctx.registry.eth_handlers().pubsub.clone()),
            Arc::new(ctx.registry.eth_api().provider().clone()),
            Box::new(ctx.node().task_executor().clone()),
        )
        .into_rpc(),
    )?;
    Ok(())
}
