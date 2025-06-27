//! A placeholder for the transaction pool.
//!
//! We'll not use transaction pool in HL, since this node does not write to HL consensus;
//! just a minimum implementation to make the node compile would suffice.
//!
//! Ethereum transaction pool only supports TransactionSigned (EthereumTxEnvelope<TxEip4844>),
//! hence this placeholder for the transaction pool.

use crate::node::{primitives::TransactionSigned, HlNode};
use alloy_consensus::{
    error::ValueError, EthereumTxEnvelope, Transaction as TransactionTrait, TxEip4844,
};
use alloy_eips::{
    eip4844::BlobAndProofV2, eip7594::BlobTransactionSidecarVariant, eip7702::SignedAuthorization,
    Typed2718,
};
use alloy_primitives::{Address, Bytes, ChainId, TxHash, TxKind, B256, U256};
use alloy_rpc_types::AccessList;
use alloy_rpc_types_engine::BlobAndProofV1;
use reth::{
    api::FullNodeTypes,
    builder::components::PoolBuilder,
    transaction_pool::{PoolResult, PoolSize, PoolTransaction, TransactionOrigin, TransactionPool},
};
use reth_eth_wire::HandleMempoolData;
use reth_ethereum_primitives::PooledTransactionVariant;
use reth_primitives::Recovered;
use reth_primitives_traits::InMemorySize;
use reth_transaction_pool::{
    error::InvalidPoolTransactionError, AllPoolTransactions, AllTransactionsEvents, BestTransactions, BestTransactionsAttributes, BlobStoreError, BlockInfo, EthPoolTransaction, GetPooledTransactionLimit, NewBlobSidecar, NewTransactionEvent, PropagatedTransactions, TransactionEvents, TransactionListenerKind, ValidPoolTransaction
};
use std::{collections::HashSet, sync::Arc};
use tokio::sync::mpsc::{self, Receiver};

pub struct HlPoolBuilder;
impl<Node> PoolBuilder<Node> for HlPoolBuilder
where
    Node: FullNodeTypes<Types = HlNode>,
{
    type Pool = HlTransactionPool;

    async fn build_pool(
        self,
        _ctx: &reth::builder::BuilderContext<Node>,
    ) -> eyre::Result<Self::Pool> {
        Ok(HlTransactionPool)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HlPooledTransaction;

impl InMemorySize for HlPooledTransaction {
    fn size(&self) -> usize {
        0
    }
}

impl TransactionTrait for HlPooledTransaction {
    fn chain_id(&self) -> Option<ChainId> {
        unreachable!()
    }
    fn nonce(&self) -> u64 {
        unreachable!()
    }
    fn gas_limit(&self) -> u64 {
        unreachable!()
    }
    fn gas_price(&self) -> Option<u128> {
        unreachable!()
    }
    fn max_fee_per_gas(&self) -> u128 {
        unreachable!()
    }
    fn max_priority_fee_per_gas(&self) -> Option<u128> {
        unreachable!()
    }
    fn max_fee_per_blob_gas(&self) -> Option<u128> {
        unreachable!()
    }
    fn priority_fee_or_price(&self) -> u128 {
        unreachable!()
    }
    fn effective_gas_price(&self, _base_fee: Option<u64>) -> u128 {
        unreachable!()
    }
    fn is_dynamic_fee(&self) -> bool {
        unreachable!()
    }
    fn kind(&self) -> TxKind {
        unreachable!()
    }
    fn is_create(&self) -> bool {
        unreachable!()
    }
    fn value(&self) -> U256 {
        unreachable!()
    }
    fn input(&self) -> &Bytes {
        unreachable!()
    }
    fn access_list(&self) -> Option<&AccessList> {
        unreachable!()
    }
    fn blob_versioned_hashes(&self) -> Option<&[B256]> {
        unreachable!()
    }
    fn authorization_list(&self) -> Option<&[SignedAuthorization]> {
        unreachable!()
    }
}

impl Typed2718 for HlPooledTransaction {
    fn ty(&self) -> u8 {
        unreachable!()
    }
}

impl PoolTransaction for HlPooledTransaction {
    type TryFromConsensusError = ValueError<EthereumTxEnvelope<TxEip4844>>;
    type Consensus = TransactionSigned;
    type Pooled = PooledTransactionVariant;

    fn try_from_consensus(
        _tx: Recovered<Self::Consensus>,
    ) -> Result<Self, Self::TryFromConsensusError> {
        unreachable!()
    }

    fn clone_into_consensus(&self) -> Recovered<Self::Consensus> {
        unreachable!()
    }

    fn into_consensus(self) -> Recovered<Self::Consensus> {
        unreachable!()
    }

    fn from_pooled(_tx: Recovered<Self::Pooled>) -> Self {
        unreachable!()
    }

    fn hash(&self) -> &TxHash {
        unreachable!()
    }

    fn sender(&self) -> Address {
        unreachable!()
    }

    fn sender_ref(&self) -> &Address {
        unreachable!()
    }

    fn cost(&self) -> &U256 {
        unreachable!()
    }

    fn encoded_length(&self) -> usize {
        0
    }

    fn ensure_max_init_code_size(
        &self,
        _max_init_code_size: usize,
    ) -> Result<(), InvalidPoolTransactionError> {
        Ok(())
    }
}

impl EthPoolTransaction for HlPooledTransaction {
    fn take_blob(&mut self) -> reth_transaction_pool::EthBlobTransactionSidecar {
        unreachable!()
    }

    fn try_into_pooled_eip4844(
        self,
        _sidecar: Arc<alloy_eips::eip7594::BlobTransactionSidecarVariant>,
    ) -> Option<Recovered<Self::Pooled>> {
        None
    }

    fn try_from_eip4844(
        _tx: Recovered<Self::Consensus>,
        _sidecar: alloy_eips::eip7594::BlobTransactionSidecarVariant,
    ) -> Option<Self> {
        None
    }

    fn validate_blob(
        &self,
        _blob: &alloy_eips::eip7594::BlobTransactionSidecarVariant,
        _settings: &alloy_eips::eip4844::env_settings::KzgSettings,
    ) -> Result<(), alloy_consensus::BlobTransactionValidationError> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HlTransactionPool;
impl TransactionPool for HlTransactionPool {
    type Transaction = HlPooledTransaction;

    fn pool_size(&self) -> PoolSize {
        PoolSize::default()
    }

    fn block_info(&self) -> BlockInfo {
        BlockInfo::default()
    }

    async fn add_transaction_and_subscribe(
        &self,
        _origin: TransactionOrigin,
        _transaction: Self::Transaction,
    ) -> PoolResult<TransactionEvents> {
        unreachable!()
    }

    async fn add_transaction(
        &self,
        _origin: TransactionOrigin,
        _transaction: Self::Transaction,
    ) -> PoolResult<TxHash> {
        Ok(TxHash::default())
    }

    async fn add_transactions(
        &self,
        _origin: TransactionOrigin,
        _transactions: Vec<Self::Transaction>,
    ) -> Vec<PoolResult<TxHash>> {
        vec![]
    }

    fn transaction_event_listener(&self, _tx_hash: TxHash) -> Option<TransactionEvents> {
        None
    }

    fn all_transactions_event_listener(&self) -> AllTransactionsEvents<Self::Transaction> {
        unreachable!()
    }

    fn pending_transactions_listener_for(
        &self,
        _kind: TransactionListenerKind,
    ) -> Receiver<TxHash> {
        mpsc::channel(1).1
    }

    fn blob_transaction_sidecars_listener(&self) -> Receiver<NewBlobSidecar> {
        mpsc::channel(1).1
    }

    fn new_transactions_listener_for(
        &self,
        _kind: TransactionListenerKind,
    ) -> Receiver<NewTransactionEvent<Self::Transaction>> {
        mpsc::channel(1).1
    }
    fn pooled_transaction_hashes(&self) -> Vec<TxHash> {
        vec![]
    }
    fn pooled_transaction_hashes_max(&self, _max: usize) -> Vec<TxHash> {
        vec![]
    }
    fn pooled_transactions(&self) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        vec![]
    }
    fn pooled_transactions_max(
        &self,
        _max: usize,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        vec![]
    }
    fn get_pooled_transaction_elements(
        &self,
        _tx_hashes: Vec<TxHash>,
        _limit: GetPooledTransactionLimit,
    ) -> Vec<<Self::Transaction as PoolTransaction>::Pooled> {
        vec![]
    }
    fn get_pooled_transaction_element(
        &self,
        _tx_hash: TxHash,
    ) -> Option<Recovered<<Self::Transaction as PoolTransaction>::Pooled>> {
        None
    }
    fn best_transactions(
        &self,
    ) -> Box<dyn BestTransactions<Item = Arc<ValidPoolTransaction<Self::Transaction>>>> {
        Box::new(std::iter::empty())
    }
    fn best_transactions_with_attributes(
        &self,
        _best_transactions_attributes: BestTransactionsAttributes,
    ) -> Box<dyn BestTransactions<Item = Arc<ValidPoolTransaction<Self::Transaction>>>> {
        Box::new(std::iter::empty())
    }
    fn pending_transactions(&self) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        vec![]
    }
    fn pending_transactions_max(
        &self,
        _max: usize,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        vec![]
    }
    fn queued_transactions(&self) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        vec![]
    }
    fn all_transactions(&self) -> AllPoolTransactions<Self::Transaction> {
        AllPoolTransactions::default()
    }
    fn remove_transactions(
        &self,
        _hashes: Vec<TxHash>,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        vec![]
    }
    fn remove_transactions_and_descendants(
        &self,
        _hashes: Vec<TxHash>,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        vec![]
    }
    fn remove_transactions_by_sender(
        &self,
        _sender: Address,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        vec![]
    }
    fn retain_unknown<A>(&self, _announcement: &mut A)
    where
        A: HandleMempoolData,
    {
        // do nothing
    }
    fn get(&self, _tx_hash: &TxHash) -> Option<Arc<ValidPoolTransaction<Self::Transaction>>> {
        None
    }
    fn get_all(&self, _txs: Vec<TxHash>) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        vec![]
    }
    fn on_propagated(&self, _txs: PropagatedTransactions) {
        // do nothing
    }
    fn get_transactions_by_sender(
        &self,
        _sender: Address,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        vec![]
    }
    fn get_pending_transactions_with_predicate(
        &self,
        _predicate: impl FnMut(&ValidPoolTransaction<Self::Transaction>) -> bool,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        vec![]
    }
    fn get_pending_transactions_by_sender(
        &self,
        _sender: Address,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        vec![]
    }
    fn get_queued_transactions_by_sender(
        &self,
        _sender: Address,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        unreachable!()
    }
    fn get_highest_transaction_by_sender(
        &self,
        _sender: Address,
    ) -> Option<Arc<ValidPoolTransaction<Self::Transaction>>> {
        None
    }
    fn get_highest_consecutive_transaction_by_sender(
        &self,
        _sender: Address,
        _on_chain_nonce: u64,
    ) -> Option<Arc<ValidPoolTransaction<Self::Transaction>>> {
        None
    }
    fn get_transaction_by_sender_and_nonce(
        &self,
        _sender: Address,
        _nonce: u64,
    ) -> Option<Arc<ValidPoolTransaction<Self::Transaction>>> {
        None
    }
    fn get_transactions_by_origin(
        &self,
        _origin: TransactionOrigin,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        unreachable!()
    }
    fn get_pending_transactions_by_origin(
        &self,
        _origin: TransactionOrigin,
    ) -> Vec<Arc<ValidPoolTransaction<Self::Transaction>>> {
        unreachable!()
    }
    fn unique_senders(&self) -> HashSet<Address> {
        unreachable!()
    }
    fn get_blob(
        &self,
        _tx_hash: TxHash,
    ) -> Result<Option<Arc<BlobTransactionSidecarVariant>>, BlobStoreError> {
        unreachable!()
    }
    fn get_all_blobs(
        &self,
        _tx_hashes: Vec<TxHash>,
    ) -> Result<Vec<(TxHash, Arc<BlobTransactionSidecarVariant>)>, BlobStoreError> {
        unreachable!()
    }
    fn get_all_blobs_exact(
        &self,
        _tx_hashes: Vec<TxHash>,
    ) -> Result<Vec<Arc<BlobTransactionSidecarVariant>>, BlobStoreError> {
        unreachable!()
    }
    fn get_blobs_for_versioned_hashes_v1(
        &self,
        _versioned_hashes: &[B256],
    ) -> Result<Vec<Option<BlobAndProofV1>>, BlobStoreError> {
        unreachable!()
    }
    fn get_blobs_for_versioned_hashes_v2(
        &self,
        _versioned_hashes: &[B256],
    ) -> Result<Option<Vec<BlobAndProofV2>>, BlobStoreError> {
        unreachable!()
    }
}
