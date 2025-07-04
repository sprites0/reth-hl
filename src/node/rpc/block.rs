use std::{future::Future, sync::Arc};

use crate::{
    chainspec::HlChainSpec,
    node::{
        primitives::TransactionSigned,
        rpc::{HlEthApi, HlNodeCore},
    },
    HlBlock,
};
use alloy_consensus::{BlockHeader, ReceiptEnvelope, TxType};
use alloy_primitives::B256;
use reth::{
    api::NodeTypes,
    builder::FullNodeComponents,
    primitives::{Receipt, SealedHeader, TransactionMeta},
    providers::{BlockReaderIdExt, ProviderHeader, ReceiptProvider, TransactionsProvider},
    rpc::{
        eth::EthApiTypes,
        server_types::eth::{
            error::FromEvmError, receipt::build_receipt, EthApiError, PendingBlock,
        },
        types::{BlockId, TransactionReceipt},
    },
    transaction_pool::{PoolTransaction, TransactionPool},
};
use reth_chainspec::{EthChainSpec, EthereumHardforks};
use reth_evm::{ConfigureEvm, NextBlockEnvAttributes};
use reth_primitives::{NodePrimitives, SealedBlock};
use reth_primitives_traits::{BlockBody as _, RecoveredBlock, SignedTransaction as _};
use reth_provider::{
    BlockIdReader, BlockReader, ChainSpecProvider, HeaderProvider, ProviderBlock, ProviderReceipt,
    ProviderTx, StateProviderFactory,
};
use reth_rpc_eth_api::{
    helpers::{EthBlocks, LoadBlock, LoadPendingBlock, LoadReceipt, SpawnBlocking},
    types::RpcTypes,
    FromEthApiError, RpcConvert, RpcNodeCore, RpcNodeCoreExt, RpcReceipt,
};

fn is_system_tx(tx: &TransactionSigned) -> bool {
    tx.is_system_transaction()
}

impl<N> EthBlocks for HlEthApi<N>
where
    Self: LoadBlock<
        Error = EthApiError,
        NetworkTypes: RpcTypes<Receipt = TransactionReceipt>,
        Provider: BlockReader<Transaction = TransactionSigned, Receipt = Receipt>,
    >,
    N: HlNodeCore<Provider: ChainSpecProvider<ChainSpec = HlChainSpec> + HeaderProvider>,
{
    async fn block_receipts(
        &self,
        block_id: BlockId,
    ) -> Result<Option<Vec<RpcReceipt<Self::NetworkTypes>>>, Self::Error>
    where
        Self: LoadReceipt,
    {
        if let Some((block, receipts)) = self.load_block_and_receipts(block_id).await? {
            let block_number = block.number();
            let base_fee = block.base_fee_per_gas();
            let block_hash = block.hash();
            let excess_blob_gas = block.excess_blob_gas();
            let timestamp = block.timestamp();
            let blob_params = self.provider().chain_spec().blob_params_at_timestamp(timestamp);

            return block
                .body()
                .transactions()
                .iter()
                .zip(receipts.iter())
                .filter(|(tx, _)| !is_system_tx(tx))
                .enumerate()
                .map(|(idx, (tx, receipt))| {
                    let meta = TransactionMeta {
                        tx_hash: *tx.tx_hash(),
                        index: idx as u64,
                        block_hash,
                        block_number,
                        base_fee,
                        excess_blob_gas,
                        timestamp,
                    };
                    build_receipt(tx, meta, receipt, &receipts, blob_params, |receipt_with_bloom| {
                        match receipt.tx_type {
                            TxType::Legacy => ReceiptEnvelope::Legacy(receipt_with_bloom),
                            TxType::Eip2930 => ReceiptEnvelope::Eip2930(receipt_with_bloom),
                            TxType::Eip1559 => ReceiptEnvelope::Eip1559(receipt_with_bloom),
                            TxType::Eip4844 => ReceiptEnvelope::Eip4844(receipt_with_bloom),
                            TxType::Eip7702 => ReceiptEnvelope::Eip7702(receipt_with_bloom),
                        }
                    })
                })
                .collect::<Result<Vec<_>, Self::Error>>()
                .map(Some);
        }

        Ok(None)
    }
}

impl<N> LoadBlock for HlEthApi<N>
where
    Self: LoadPendingBlock
        + SpawnBlocking
        + RpcNodeCoreExt<
            Pool: TransactionPool<
                Transaction: PoolTransaction<Consensus = ProviderTx<Self::Provider>>,
            >,
        > + RpcNodeCore<Provider: BlockReader<Block = crate::HlBlock>>,
    N: HlNodeCore,
{
    fn recovered_block(
        &self,
        block_id: BlockId,
    ) -> impl Future<
        Output = Result<
            Option<Arc<RecoveredBlock<<Self::Provider as BlockReader>::Block>>>,
            Self::Error,
        >,
    > + Send {
        let hl_node_compliant = self.hl_node_compliant;
        async move {
            // Copy of LoadBlock::recovered_block, but with --hl-node-compliant support
            if block_id.is_pending() {
                return Ok(None);
            }

            let block_hash = match self
                .provider()
                .block_hash_for_id(block_id)
                .map_err(Self::Error::from_eth_err)?
            {
                Some(block_hash) => block_hash,
                None => return Ok(None),
            };

            let recovered_block = self
                .cache()
                .get_recovered_block(block_hash)
                .await
                .map_err(Self::Error::from_eth_err)?;

            if let Some(recovered_block) = recovered_block {
                let recovered_block = if hl_node_compliant {
                    filter_if_hl_node_compliant(&recovered_block)
                } else {
                    (*recovered_block).clone()
                };
                return Ok(Some(std::sync::Arc::new(recovered_block)));
            }

            Ok(None)
        }
    }
}

fn filter_if_hl_node_compliant(
    recovered_block: &RecoveredBlock<HlBlock>,
) -> RecoveredBlock<HlBlock> {
    let sealed_block = recovered_block.sealed_block();
    let transactions = sealed_block.body().transactions();
    let to_skip = transactions
        .iter()
        .position(|tx| !tx.is_system_transaction())
        .unwrap_or(transactions.len());

    let mut new_block: HlBlock = sealed_block.clone_block();
    new_block.body.transactions.drain(..to_skip);
    let new_sealed_block = SealedBlock::new_unchecked(new_block, sealed_block.hash());
    let new_senders = recovered_block.senders()[to_skip..].to_vec();

    RecoveredBlock::new_sealed(new_sealed_block, new_senders)
}

impl<N> LoadPendingBlock for HlEthApi<N>
where
    Self: SpawnBlocking
        + EthApiTypes<
            NetworkTypes: RpcTypes<
                Header = alloy_rpc_types_eth::Header<ProviderHeader<Self::Provider>>,
            >,
            Error: FromEvmError<Self::Evm>,
            RpcConvert: RpcConvert<Network = Self::NetworkTypes>,
        >,
    N: RpcNodeCore<
        Provider: BlockReaderIdExt
                      + ChainSpecProvider<ChainSpec: EthChainSpec + EthereumHardforks>
                      + StateProviderFactory,
        Pool: TransactionPool<Transaction: PoolTransaction<Consensus = ProviderTx<N::Provider>>>,
        Evm: ConfigureEvm<
            Primitives = <Self as RpcNodeCore>::Primitives,
            NextBlockEnvCtx: From<NextBlockEnvAttributes>,
        >,
        Primitives: NodePrimitives<
            BlockHeader = ProviderHeader<Self::Provider>,
            SignedTx = ProviderTx<Self::Provider>,
            Receipt = ProviderReceipt<Self::Provider>,
            Block = ProviderBlock<Self::Provider>,
        >,
    >,
{
    #[inline]
    fn pending_block(
        &self,
    ) -> &tokio::sync::Mutex<
        Option<PendingBlock<ProviderBlock<Self::Provider>, ProviderReceipt<Self::Provider>>>,
    > {
        self.inner.eth_api.pending_block()
    }

    fn next_env_attributes(
        &self,
        parent: &SealedHeader<ProviderHeader<Self::Provider>>,
    ) -> Result<<Self::Evm as reth_evm::ConfigureEvm>::NextBlockEnvCtx, Self::Error> {
        Ok(NextBlockEnvAttributes {
            timestamp: parent.timestamp().saturating_add(12),
            suggested_fee_recipient: parent.beneficiary(),
            prev_randao: B256::random(),
            gas_limit: parent.gas_limit(),
            parent_beacon_block_root: parent.parent_beacon_block_root(),
            withdrawals: None,
        }
        .into())
    }
}

impl<N> LoadReceipt for HlEthApi<N>
where
    Self: Send + Sync,
    N: FullNodeComponents<Types: NodeTypes<ChainSpec = HlChainSpec>>,
    Self::Provider:
        TransactionsProvider<Transaction = TransactionSigned> + ReceiptProvider<Receipt = Receipt>,
{
    async fn build_transaction_receipt(
        &self,
        tx: TransactionSigned,
        meta: TransactionMeta,
        receipt: Receipt,
    ) -> Result<RpcReceipt<Self::NetworkTypes>, Self::Error> {
        let hash = meta.block_hash;
        // get all receipts for the block
        let all_receipts = self
            .cache()
            .get_receipts(hash)
            .await
            .map_err(Self::Error::from_eth_err)?
            .ok_or(EthApiError::HeaderNotFound(hash.into()))?;
        let blob_params = self.provider().chain_spec().blob_params_at_timestamp(meta.timestamp);

        build_receipt(&tx, meta, &receipt, &all_receipts, blob_params, |receipt_with_bloom| {
            match receipt.tx_type {
                TxType::Legacy => ReceiptEnvelope::Legacy(receipt_with_bloom),
                TxType::Eip2930 => ReceiptEnvelope::Eip2930(receipt_with_bloom),
                TxType::Eip1559 => ReceiptEnvelope::Eip1559(receipt_with_bloom),
                TxType::Eip4844 => ReceiptEnvelope::Eip4844(receipt_with_bloom),
                TxType::Eip7702 => ReceiptEnvelope::Eip7702(receipt_with_bloom),
            }
        })
    }
}
