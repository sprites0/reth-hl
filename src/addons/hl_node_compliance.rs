//! Overrides for RPC methods to post-filter system transactions and logs.
//!
//! System transactions are always at the beginning of the block,
//! so we can use the transaction index to determine if the log is from a system transaction,
//! and if it is, we can exclude it.
//!
//! For non-system transactions, we can just return the log as is, and the client will
//! adjust the transaction index accordingly.

use alloy_consensus::{transaction::TransactionMeta, BlockHeader, TxReceipt};
use alloy_eips::{BlockId, BlockNumberOrTag};
use alloy_json_rpc::RpcObject;
use alloy_primitives::{B256, U256};
use alloy_rpc_types::{
    pubsub::{Params, SubscriptionKind},
    BlockTransactions, Filter, FilterChanges, FilterId, Log, PendingTransactionFilterKind,
    TransactionInfo,
};
use jsonrpsee::{proc_macros::rpc, PendingSubscriptionSink, SubscriptionMessage, SubscriptionSink};
use jsonrpsee_core::{async_trait, RpcResult};
use jsonrpsee_types::ErrorObject;
use reth::{api::FullNodeComponents, builder::rpc::RpcContext, tasks::TaskSpawner};
use reth_primitives_traits::{BlockBody as _, SignedTransaction};
use reth_provider::{BlockIdReader, BlockReader, BlockReaderIdExt, ReceiptProvider};
use reth_rpc::{eth::pubsub::SubscriptionSerializeError, EthFilter, EthPubSub, RpcTypes};
use reth_rpc_eth_api::{
    helpers::{EthBlocks, EthTransactions, LoadReceipt},
    transaction::ConvertReceiptInput,
    EthApiServer, EthApiTypes, EthFilterApiServer, EthPubSubApiServer, FullEthApiTypes, RpcBlock,
    RpcConvert, RpcHeader, RpcNodeCoreExt, RpcReceipt, RpcTransaction, RpcTxReq,
};
use serde::Serialize;
use std::{borrow::Cow, marker::PhantomData, sync::Arc};
use tokio_stream::{Stream, StreamExt};
use tracing::{trace, Instrument};

use crate::{node::primitives::HlPrimitives, HlBlock};

pub trait EthWrapper:
    EthApiServer<
        RpcTxReq<Self::NetworkTypes>,
        RpcTransaction<Self::NetworkTypes>,
        RpcBlock<Self::NetworkTypes>,
        RpcReceipt<Self::NetworkTypes>,
        RpcHeader<Self::NetworkTypes>,
    > + FullEthApiTypes<
        Primitives = HlPrimitives,
        NetworkTypes: RpcTypes<TransactionResponse = alloy_rpc_types_eth::Transaction>,
    > + RpcNodeCoreExt<Provider: BlockReader<Block = HlBlock>>
    + EthBlocks
    + EthTransactions
    + LoadReceipt
    + 'static
{
}

impl<T> EthWrapper for T where
    T: EthApiServer<
            RpcTxReq<Self::NetworkTypes>,
            RpcTransaction<Self::NetworkTypes>,
            RpcBlock<Self::NetworkTypes>,
            RpcReceipt<Self::NetworkTypes>,
            RpcHeader<Self::NetworkTypes>,
        > + FullEthApiTypes<
            Primitives = HlPrimitives,
            NetworkTypes: RpcTypes<TransactionResponse = alloy_rpc_types_eth::Transaction>,
        > + RpcNodeCoreExt<Provider: BlockReader<Block = HlBlock>>
        + EthBlocks
        + EthTransactions
        + LoadReceipt
        + 'static
{
}

#[rpc(server, namespace = "eth")]
#[async_trait]
pub trait EthSystemTransactionApi<T: RpcObject> {
    #[method(name = "getSystemTxsByBlockHash")]
    async fn get_system_txs_by_block_hash(&self, hash: B256) -> RpcResult<Option<Vec<T>>>;

    #[method(name = "getSystemTxsByBlockNumber")]
    async fn get_system_txs_by_block_number(
        &self,
        block_id: Option<BlockId>,
    ) -> RpcResult<Option<Vec<T>>>;
}

pub struct HlSystemTransactionExt<Eth: EthWrapper> {
    eth_api: Eth,
    _marker: PhantomData<Eth>,
}

impl<Eth: EthWrapper> HlSystemTransactionExt<Eth> {
    pub fn new(eth_api: Eth) -> Self {
        Self { eth_api, _marker: PhantomData }
    }
}

#[async_trait]
impl<Eth: EthWrapper> EthSystemTransactionApiServer<RpcTransaction<Eth::NetworkTypes>>
    for HlSystemTransactionExt<Eth>
where
    jsonrpsee_types::ErrorObject<'static>: From<<Eth as EthApiTypes>::Error>,
{
    /// Returns the system transactions for a given block hash.
    /// Compliance with the `eth_getSystemTxsByBlockHash` RPC method introduced by hl-node.
    /// https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/hyperevm/json-rpc
    async fn get_system_txs_by_block_hash(
        &self,
        hash: B256,
    ) -> RpcResult<Option<Vec<RpcTransaction<Eth::NetworkTypes>>>> {
        trace!(target: "rpc::eth", ?hash, "Serving eth_getSystemTxsByBlockHash");
        self.get_system_txs_by_block_number(Some(BlockId::Hash(hash.into()))).await
    }

    /// Returns the system transactions for a given block number, or the latest block if no block
    /// number is provided. Compliance with the `eth_getSystemTxsByBlockNumber` RPC method
    /// introduced by hl-node. https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/hyperevm/json-rpc
    async fn get_system_txs_by_block_number(
        &self,
        id: Option<BlockId>,
    ) -> RpcResult<Option<Vec<RpcTransaction<Eth::NetworkTypes>>>> {
        trace!(target: "rpc::eth", ?id, "Serving eth_getSystemTxsByBlockNumber");

        if let Some(block) = self.eth_api.recovered_block(id.unwrap_or_default()).await? {
            let block_hash = block.hash();
            let block_number = block.number();
            let base_fee_per_gas = block.base_fee_per_gas();
            let system_txs = block
                .transactions_with_sender()
                .enumerate()
                .filter_map(|(index, (signer, tx))| {
                    if tx.is_system_transaction() {
                        let tx_info = TransactionInfo {
                            hash: Some(*tx.tx_hash()),
                            block_hash: Some(block_hash),
                            block_number: Some(block_number),
                            base_fee: base_fee_per_gas,
                            index: Some(index as u64),
                        };
                        self.eth_api
                            .tx_resp_builder()
                            .fill(tx.clone().with_signer(*signer), tx_info)
                            .ok()
                    } else {
                        None
                    }
                })
                .collect();
            Ok(Some(system_txs))
        } else {
            Ok(None)
        }
    }
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
    async fn new_filter(&self, filter: Filter) -> RpcResult<FilterId> {
        trace!(target: "rpc::eth", "Serving eth_newFilter");
        self.filter.new_filter(filter).await
    }

    async fn new_block_filter(&self) -> RpcResult<FilterId> {
        trace!(target: "rpc::eth", "Serving eth_newBlockFilter");
        self.filter.new_block_filter().await
    }

    async fn new_pending_transaction_filter(
        &self,
        kind: Option<PendingTransactionFilterKind>,
    ) -> RpcResult<FilterId> {
        trace!(target: "rpc::eth", "Serving eth_newPendingTransactionFilter");
        self.filter.new_pending_transaction_filter(kind).await
    }

    async fn filter_changes(
        &self,
        id: FilterId,
    ) -> RpcResult<FilterChanges<RpcTransaction<Eth::NetworkTypes>>> {
        trace!(target: "rpc::eth", "Serving eth_getFilterChanges");
        self.filter.filter_changes(id).await.map_err(ErrorObject::from)
    }

    async fn filter_logs(&self, id: FilterId) -> RpcResult<Vec<Log>> {
        trace!(target: "rpc::eth", "Serving eth_getFilterLogs");
        self.filter.filter_logs(id).await.map_err(ErrorObject::from)
    }

    async fn uninstall_filter(&self, id: FilterId) -> RpcResult<bool> {
        trace!(target: "rpc::eth", "Serving eth_uninstallFilter");
        self.filter.uninstall_filter(id).await
    }

    async fn logs(&self, filter: Filter) -> RpcResult<Vec<Log>> {
        trace!(target: "rpc::eth", "Serving eth_getLogs");
        let logs = EthFilterApiServer::logs(&*self.filter, filter).await?;
        Ok(logs.into_iter().filter_map(|log| adjust_log::<Eth>(log, &self.provider)).collect())
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
impl<Eth: EthWrapper> EthPubSubApiServer<RpcTransaction<Eth::NetworkTypes>> for HlNodeFilterWs<Eth>
where
    jsonrpsee_types::error::ErrorObject<'static>: From<<Eth as EthApiTypes>::Error>,
{
    async fn subscribe(
        &self,
        pending: PendingSubscriptionSink,
        kind: SubscriptionKind,
        params: Option<Params>,
    ) -> jsonrpsee::core::SubscriptionResult {
        let sink = pending.accept().await?;
        let (pubsub, provider) = (self.pubsub.clone(), self.provider.clone());
        self.subscription_task_spawner.spawn(Box::pin(async move {
            if kind == SubscriptionKind::Logs {
                let filter = match params {
                    Some(Params::Logs(f)) => *f,
                    Some(Params::Bool(_)) => return,
                    _ => Default::default(),
                };
                let _ = pipe_from_stream(
                    sink,
                    pubsub.log_stream(filter).filter_map(|log| adjust_log::<Eth>(log, &provider)),
                )
                .await;
            } else {
                let _ = pubsub.handle_accepted(sink, kind, params).await;
            }
        }));
        Ok(())
    }
}

fn adjust_log<Eth: EthWrapper>(mut log: Log, provider: &Eth::Provider) -> Option<Log> {
    let (tx_idx, log_idx) = (log.transaction_index?, log.log_index?);
    let receipts = provider.receipts_by_block(log.block_number?.into()).unwrap()?;
    let (mut sys_tx_count, mut sys_log_count) = (0u64, 0u64);
    for receipt in receipts {
        if receipt.cumulative_gas_used() == 0 {
            sys_tx_count += 1;
            sys_log_count += receipt.logs().len() as u64;
        }
    }
    if sys_tx_count > tx_idx {
        return None;
    }
    log.transaction_index = Some(tx_idx - sys_tx_count);
    log.log_index = Some(log_idx - sys_log_count);
    Some(log)
}

async fn pipe_from_stream<T: Serialize, St: Stream<Item = T> + Unpin>(
    sink: SubscriptionSink,
    mut stream: St,
) -> Result<(), ErrorObject<'static>> {
    loop {
        tokio::select! {
            _ = sink.closed() => break Ok(()),
            maybe_item = stream.next() => {
                let Some(item) = maybe_item else { break Ok(()) };
                let msg = SubscriptionMessage::new(sink.method_name(), sink.subscription_id(), &item)
                    .map_err(SubscriptionSerializeError::from)?;
                if sink.send(msg).await.is_err() { break Ok(()); }
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

    #[method(name = "getTransactionReceipt")]
    async fn transaction_receipt(&self, hash: B256) -> RpcResult<Option<R>>;
}

macro_rules! engine_span {
    () => {
        tracing::trace_span!(target: "rpc", "engine")
    };
}

fn adjust_block<Eth: EthWrapper>(
    recovered_block: &RpcBlock<Eth::NetworkTypes>,
    eth_api: &Eth,
) -> RpcBlock<Eth::NetworkTypes> {
    let system_tx_count = system_tx_count_for_block(eth_api, recovered_block.number().into());
    let mut new_block = recovered_block.clone();

    new_block.transactions = match new_block.transactions {
        BlockTransactions::Full(mut transactions) => {
            transactions.drain(..system_tx_count);
            transactions.iter_mut().for_each(|tx| {
                if let Some(idx) = &mut tx.transaction_index {
                    *idx -= system_tx_count as u64;
                }
            });
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

async fn adjust_block_receipts<Eth: EthWrapper>(
    block_id: BlockId,
    eth_api: &Eth,
) -> Result<Option<(usize, Vec<RpcReceipt<Eth::NetworkTypes>>)>, Eth::Error> {
    // Modified from EthBlocks::block_receipt. See `NOTE` comment below.
    let system_tx_count = system_tx_count_for_block(eth_api, block_id);
    if let Some((block, receipts)) = EthBlocks::load_block_and_receipts(eth_api, block_id).await? {
        let block_number = block.number;
        let base_fee = block.base_fee_per_gas;
        let block_hash = block.hash();
        let excess_blob_gas = block.excess_blob_gas;
        let timestamp = block.timestamp;
        let mut gas_used = 0;
        let mut next_log_index = 0;

        let inputs = block
            .transactions_recovered()
            .zip(receipts.iter())
            .enumerate()
            .filter_map(|(idx, (tx, receipt))| {
                if receipt.cumulative_gas_used() == 0 {
                    // NOTE: modified to exclude system tx
                    return None;
                }
                let meta = TransactionMeta {
                    tx_hash: *tx.tx_hash(),
                    index: (idx - system_tx_count) as u64,
                    block_hash,
                    block_number,
                    base_fee,
                    excess_blob_gas,
                    timestamp,
                };

                let input = ConvertReceiptInput {
                    receipt: Cow::Borrowed(receipt),
                    tx,
                    gas_used: receipt.cumulative_gas_used() - gas_used,
                    next_log_index,
                    meta,
                };

                gas_used = receipt.cumulative_gas_used();
                next_log_index += receipt.logs().len();

                Some(input)
            })
            .collect::<Vec<_>>();

        return eth_api
            .tx_resp_builder()
            .convert_receipts(inputs)
            .map(|receipts| Some((system_tx_count, receipts)));
    }

    Ok(None)
}

async fn adjust_transaction_receipt<Eth: EthWrapper>(
    tx_hash: B256,
    eth_api: &Eth,
) -> Result<Option<RpcReceipt<Eth::NetworkTypes>>, Eth::Error> {
    match eth_api.load_transaction_and_receipt(tx_hash).await? {
        Some((_, meta, _)) => {
            // LoadReceipt::block_transaction_receipt loads the block again, so loading blocks again
            // doesn't hurt performance much
            let Some((system_tx_count, block_receipts)) =
                adjust_block_receipts(meta.block_hash.into(), eth_api).await?
            else {
                unreachable!();
            };
            Ok(Some(block_receipts.into_iter().nth(meta.index as usize - system_tx_count).unwrap()))
        }
        None => Ok(None),
    }
}

// This function assumes that `block_id` is already validated by the caller.
fn system_tx_count_for_block<Eth: EthWrapper>(eth_api: &Eth, block_id: BlockId) -> usize {
    let provider = eth_api.provider();
    let block = provider.block_by_id(block_id).unwrap().unwrap();
    let system_tx_count =
        block.body.transactions().iter().filter(|tx| tx.is_system_transaction()).count();
    system_tx_count
}

#[async_trait]
impl<Eth: EthWrapper> EthBlockApiServer<RpcBlock<Eth::NetworkTypes>, RpcReceipt<Eth::NetworkTypes>>
    for HlNodeBlockFilterHttp<Eth>
where
    Eth: EthApiTypes + 'static,
    ErrorObject<'static>: From<Eth::Error>,
{
    /// Handler for: `eth_getBlockByHash`
    async fn block_by_hash(
        &self,
        hash: B256,
        full: bool,
    ) -> RpcResult<Option<RpcBlock<Eth::NetworkTypes>>> {
        let res = self.eth_api.block_by_hash(hash, full).instrument(engine_span!()).await?;
        Ok(res.map(|block| adjust_block(&block, &*self.eth_api)))
    }

    /// Handler for: `eth_getBlockByNumber`
    async fn block_by_number(
        &self,
        number: BlockNumberOrTag,
        full: bool,
    ) -> RpcResult<Option<RpcBlock<Eth::NetworkTypes>>> {
        trace!(target: "rpc::eth", ?number, ?full, "Serving eth_getBlockByNumber");
        let res = self.eth_api.block_by_number(number, full).instrument(engine_span!()).await?;
        Ok(res.map(|block| adjust_block(&block, &*self.eth_api)))
    }

    /// Handler for: `eth_getBlockTransactionCountByHash`
    async fn block_transaction_count_by_hash(&self, hash: B256) -> RpcResult<Option<U256>> {
        trace!(target: "rpc::eth", ?hash, "Serving eth_getBlockTransactionCountByHash");
        let res =
            self.eth_api.block_transaction_count_by_hash(hash).instrument(engine_span!()).await?;
        Ok(res.map(|count| {
            let sys_tx_count =
                system_tx_count_for_block(&*self.eth_api, BlockId::Hash(hash.into()));
            count - U256::from(sys_tx_count)
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

    async fn transaction_receipt(
        &self,
        hash: B256,
    ) -> RpcResult<Option<RpcReceipt<Eth::NetworkTypes>>> {
        trace!(target: "rpc::eth", ?hash, "Serving eth_getTransactionReceipt");
        let eth_api = &*self.eth_api;
        Ok(adjust_transaction_receipt(hash, eth_api).instrument(engine_span!()).await?)
    }

    /// Handler for: `eth_getBlockReceipts`
    async fn block_receipts(
        &self,
        block_id: BlockId,
    ) -> RpcResult<Option<Vec<RpcReceipt<Eth::NetworkTypes>>>> {
        trace!(target: "rpc::eth", ?block_id, "Serving eth_getBlockReceipts");
        let result =
            adjust_block_receipts(block_id, &*self.eth_api).instrument(engine_span!()).await?;
        Ok(result.map(|(_, receipts)| receipts))
    }
}

pub fn install_hl_node_compliance<Node, EthApi>(
    ctx: RpcContext<Node, EthApi>,
) -> Result<(), eyre::Error>
where
    Node: FullNodeComponents,
    Node::Provider: BlockIdReader + BlockReader<Block = crate::HlBlock>,
    EthApi: EthWrapper,
    ErrorObject<'static>: From<EthApi::Error>,
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

    ctx.modules
        .merge_configured(HlSystemTransactionExt::new(ctx.registry.eth_api().clone()).into_rpc())?;

    Ok(())
}
