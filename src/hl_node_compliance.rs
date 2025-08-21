use alloy_consensus::TxReceipt;
use alloy_network::ReceiptResponse;
use alloy_eips::{BlockId, BlockNumberOrTag};
use alloy_json_rpc::RpcObject;
use alloy_primitives::{B256, U256};
use alloy_rpc_types::{
    pubsub::{Params, SubscriptionKind},
    BlockTransactions, Filter, FilterChanges, FilterId, Log, PendingTransactionFilterKind,
};
use jsonrpsee::{proc_macros::rpc, PendingSubscriptionSink, SubscriptionMessage, SubscriptionSink};
use jsonrpsee_core::{async_trait, RpcResult};
use jsonrpsee_types::ErrorObject;
use reth::{
    api::FullNodeComponents, builder::rpc::RpcContext, rpc::result::internal_rpc_err,
    tasks::TaskSpawner,
};
use reth_primitives_traits::BlockBody as _;
use reth_provider::{BlockIdReader, BlockReader, BlockReaderIdExt, ReceiptProvider};
use reth_rpc::{EthFilter, EthPubSub};
use reth_rpc_eth_api::{
    EthApiServer, EthApiTypes, EthFilterApiServer, EthPubSubApiServer, FullEthApiTypes, RpcBlock,
    RpcHeader, RpcNodeCore, RpcNodeCoreExt, RpcReceipt, RpcTransaction, RpcTxReq,
};
use serde::Serialize;
use std::{marker::PhantomData, sync::Arc};
use tokio_stream::{Stream, StreamExt};
use tracing::{trace, Instrument};

use crate::{
    node::primitives::{HlPrimitives, TransactionSigned},
    HlBlock,
};

pub trait EthWrapper:
    EthApiServer<
        RpcTxReq<Self::NetworkTypes>,
        RpcTransaction<Self::NetworkTypes>,
        RpcBlock<Self::NetworkTypes>,
        RpcReceipt<Self::NetworkTypes>,
        RpcHeader<Self::NetworkTypes>,
    > + FullEthApiTypes<Primitives = HlPrimitives>
    + RpcNodeCoreExt<Provider: BlockReader<Block = HlBlock>>
    + 'static
{
}

impl<
        T: EthApiServer<
                RpcTxReq<Self::NetworkTypes>,
                RpcTransaction<Self::NetworkTypes>,
                RpcBlock<Self::NetworkTypes>,
                RpcReceipt<Self::NetworkTypes>,
                RpcHeader<Self::NetworkTypes>,
            > + FullEthApiTypes<Primitives = HlPrimitives>
            + RpcNodeCoreExt
            + RpcNodeCore<Provider: BlockReader<Block = HlBlock>>
            + 'static,
    > EthWrapper for T
{
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
        let logs = EthFilterApiServer::logs(&*self.filter, filter).await?;
        let provider = self.provider.clone();

        Ok(logs.into_iter().filter_map(|log| exclude_system_tx::<Eth>(log, &provider)).collect())
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
                        .filter_map(|log| exclude_system_tx::<Eth>(log, &provider)),
                )
                .await;
            } else {
                let _ = pubsub.handle_accepted(sink, kind, params).await;
            };
        }));

        Ok(())
    }
}

fn exclude_system_tx<Eth: EthWrapper>(mut log: Log, provider: &Eth::Provider) -> Option<Log> {
    let transaction_index = log.transaction_index?;
    let log_index = log.log_index?;

    let receipts = provider.receipts_by_block(log.block_number?.into()).unwrap()?;

    // System transactions are always at the beginning of the block,
    // so we can use the transaction index to determine if the log is from a system transaction,
    // and if it is, we can exclude it.
    //
    // For non-system transactions, we can just return the log as is, and the client will
    // adjust the transaction index accordingly.
    let mut system_tx_count = 0u64;
    let mut system_tx_logs_count = 0u64;

    for receipt in receipts {
        let is_system_tx = receipt.cumulative_gas_used() == 0;
        if is_system_tx {
            system_tx_count += 1;
            system_tx_logs_count += receipt.logs().len() as u64;
        }
    }

    if system_tx_count > transaction_index {
        return None;
    }

    log.transaction_index = Some(transaction_index - system_tx_count);
    log.log_index = Some(log_index - system_tx_logs_count);
    Some(log)
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

pub struct HlNodeBlockFilterHttp<Eth: EthWrapper> {
    eth_api: Arc<Eth>,
    _marker: PhantomData<Eth>,
}

impl<Eth: EthWrapper> HlNodeBlockFilterHttp<Eth> {
    pub fn new(eth_api: Arc<Eth>) -> Self {
        Self { eth_api, _marker: PhantomData }
    }
}

#[rpc(server, namespace = "eth")]
pub trait EthBlockApi<B: RpcObject, R: RpcObject> {
    /// Returns information about a block by hash.
    #[method(name = "getBlockByHash")]
    async fn block_by_hash(&self, hash: B256, full: bool) -> RpcResult<Option<B>>;

    /// Returns information about a block by number.
    #[method(name = "getBlockByNumber")]
    async fn block_by_number(&self, number: BlockNumberOrTag, full: bool) -> RpcResult<Option<B>>;

    /// Returns all transaction receipts for a given block.
    #[method(name = "getBlockReceipts")]
    async fn block_receipts(&self, block_id: BlockId) -> RpcResult<Option<Vec<R>>>;

    #[method(name = "getBlockTransactionCountByHash")]
    async fn block_transaction_count_by_hash(&self, hash: B256) -> RpcResult<Option<U256>>;

    #[method(name = "getBlockTransactionCountByNumber")]
    async fn block_transaction_count_by_number(
        &self,
        number: BlockNumberOrTag,
    ) -> RpcResult<Option<U256>>;
}

macro_rules! engine_span {
    () => {
        tracing::trace_span!(target: "rpc", "engine")
    };
}

fn is_system_tx(tx: &TransactionSigned) -> bool {
    tx.is_system_transaction()
}

fn exclude_system_tx_from_block<Eth: EthWrapper>(
    recovered_block: &RpcBlock<Eth::NetworkTypes>,
    eth_api: &Eth,
) -> RpcBlock<Eth::NetworkTypes> {
    let system_tx_count = system_tx_count_for_block(eth_api, recovered_block.number().into());
    let mut new_block = recovered_block.clone();

    new_block.transactions = match new_block.transactions {
        BlockTransactions::Full(mut transactions) => {
            transactions.drain(..system_tx_count);
            BlockTransactions::Full(transactions)
        }
        BlockTransactions::Hashes(mut hashes) => {
            hashes.drain(..system_tx_count);
            BlockTransactions::Hashes(hashes)
        }
        BlockTransactions::Uncle => BlockTransactions::Uncle,
    };
    new_block
}

fn system_tx_count_for_block<Eth: EthWrapper>(eth_api: &Eth, number: BlockId) -> usize {
    let provider = eth_api.provider();
    let block = provider.block_by_id(number).unwrap().unwrap();
    let system_tx_count = block.body.transactions().iter().filter(|tx| is_system_tx(tx)).count();
    system_tx_count
}

#[async_trait]
impl<Eth: EthWrapper> EthBlockApiServer<RpcBlock<Eth::NetworkTypes>, RpcReceipt<Eth::NetworkTypes>>
    for HlNodeBlockFilterHttp<Eth>
where
    Eth: EthApiTypes + 'static,
{
    /// Handler for: `eth_getBlockByHash`
    async fn block_by_hash(
        &self,
        hash: B256,
        full: bool,
    ) -> RpcResult<Option<RpcBlock<Eth::NetworkTypes>>> {
        let res = self.eth_api.block_by_hash(hash, full).instrument(engine_span!()).await?;
        Ok(res.map(|block| exclude_system_tx_from_block(&block, &*self.eth_api)))
    }

    /// Handler for: `eth_getBlockByNumber`
    async fn block_by_number(
        &self,
        number: BlockNumberOrTag,
        full: bool,
    ) -> RpcResult<Option<RpcBlock<Eth::NetworkTypes>>> {
        trace!(target: "rpc::eth", ?number, ?full, "Serving eth_getBlockByNumber");
        let res = self.eth_api.block_by_number(number, full).instrument(engine_span!()).await?;
        Ok(res.map(|block| exclude_system_tx_from_block(&block, &*self.eth_api)))
    }

    /// Handler for: `eth_getBlockTransactionCountByHash`
    async fn block_transaction_count_by_hash(&self, hash: B256) -> RpcResult<Option<U256>> {
        trace!(target: "rpc::eth", ?hash, "Serving eth_getBlockTransactionCountByHash");
        let res =
            self.eth_api.block_transaction_count_by_hash(hash).instrument(engine_span!()).await?;
        Ok(res.map(|count| {
            count
                - U256::from(system_tx_count_for_block(&*self.eth_api, BlockId::Hash(hash.into())))
        }))
    }

    /// Handler for: `eth_getBlockTransactionCountByNumber`
    async fn block_transaction_count_by_number(
        &self,
        number: BlockNumberOrTag,
    ) -> RpcResult<Option<U256>> {
        trace!(target: "rpc::eth", ?number, "Serving eth_getBlockTransactionCountByNumber");
        let res = self
            .eth_api
            .block_transaction_count_by_number(number)
            .instrument(engine_span!())
            .await?;
        Ok(res.map(|count| {
            count - U256::from(system_tx_count_for_block(&*self.eth_api, number.into()))
        }))
    }

    /// Handler for: `eth_getBlockReceipts`
    async fn block_receipts(
        &self,
        block_id: BlockId,
    ) -> RpcResult<Option<Vec<RpcReceipt<Eth::NetworkTypes>>>> {
        trace!(target: "rpc::eth", ?block_id, "Serving eth_getBlockReceipts");
        let res = self.eth_api.block_receipts(block_id).instrument(engine_span!()).await?;
        Ok(res.map(|receipts| {
            receipts
                .into_iter()
                .filter(|receipt| {
                    receipt.cumulative_gas_used() > 0
                })
                .collect()
        }))
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

    ctx.modules.replace_configured(
        HlNodeBlockFilterHttp::new(Arc::new(ctx.registry.eth_api().clone())).into_rpc(),
    )?;
    Ok(())
}
