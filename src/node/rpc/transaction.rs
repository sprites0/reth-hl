use super::HlNodeCore;
use crate::node::rpc::HlEthApi;
use alloy_primitives::{Bytes, B256};
use reth::{
    rpc::server_types::eth::utils::recover_raw_transaction,
    transaction_pool::{PoolTransaction, TransactionOrigin, TransactionPool},
};
use reth_provider::{BlockReader, BlockReaderIdExt, ProviderTx, TransactionsProvider};
use reth_rpc_eth_api::{
    helpers::{EthSigner, EthTransactions, LoadTransaction, SpawnBlocking},
    FromEthApiError, FullEthApiTypes, RpcNodeCore, RpcNodeCoreExt,
};

impl<N> LoadTransaction for HlEthApi<N>
where
    Self: SpawnBlocking + FullEthApiTypes + RpcNodeCoreExt,
    N: HlNodeCore<Provider: TransactionsProvider, Pool: TransactionPool>,
    Self::Pool: TransactionPool,
{
}

impl<N> EthTransactions for HlEthApi<N>
where
    Self: LoadTransaction<Provider: BlockReaderIdExt>,
    N: HlNodeCore<Provider: BlockReader<Transaction = ProviderTx<Self::Provider>>>,
{
    fn signers(&self) -> &parking_lot::RwLock<Vec<Box<dyn EthSigner<ProviderTx<Self::Provider>>>>> {
        self.inner.eth_api.signers()
    }

    /// Decodes and recovers the transaction and submits it to the pool.
    ///
    /// Returns the hash of the transaction.
    async fn send_raw_transaction(&self, tx: Bytes) -> Result<B256, Self::Error> {
        let recovered = recover_raw_transaction(&tx)?;

        // broadcast raw transaction to subscribers if there is any.
        self.inner.eth_api.broadcast_raw_transaction(tx);

        let pool_transaction = <Self::Pool as TransactionPool>::Transaction::from_pooled(recovered);

        // submit the transaction to the pool with a `Local` origin
        let hash = self
            .pool()
            .add_transaction(TransactionOrigin::Local, pool_transaction)
            .await
            .map_err(Self::Error::from_eth_err)?;

        Ok(hash)
    }
}
