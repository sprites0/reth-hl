use super::{HlEthApi, HlNodeCore};
use crate::evm::transaction::HlTxEnv;
use alloy_rpc_types::TransactionRequest;
use reth::rpc::server_types::eth::EthApiError;
use reth_evm::{block::BlockExecutorFactory, ConfigureEvm, EvmFactory, TxEnvFor};
use reth_primitives::NodePrimitives;
use reth_provider::{ProviderError, ProviderHeader, ProviderTx};
use reth_rpc_eth_api::{
    helpers::{estimate::EstimateCall, Call, EthCall, LoadBlock, LoadState, SpawnBlocking},
    FromEvmError, FullEthApiTypes, RpcConvert, RpcTypes,
};
use revm::context::TxEnv;

impl<N> EthCall for HlEthApi<N>
where
    Self: EstimateCall + LoadBlock + FullEthApiTypes,
    N: HlNodeCore,
{
}

impl<N> EstimateCall for HlEthApi<N>
where
    Self: Call,
    Self::Error: From<EthApiError>,
    N: HlNodeCore,
{
}

impl<N> Call for HlEthApi<N>
where
    Self: LoadState<
            Evm: ConfigureEvm<
                Primitives: NodePrimitives<
                    BlockHeader = ProviderHeader<Self::Provider>,
                    SignedTx = ProviderTx<Self::Provider>,
                >,
                BlockExecutorFactory: BlockExecutorFactory<
                    EvmFactory: EvmFactory<Tx = HlTxEnv<TxEnv>>,
                >,
            >,
            RpcConvert: RpcConvert<TxEnv = TxEnvFor<Self::Evm>, Network = Self::NetworkTypes>,
            NetworkTypes: RpcTypes<TransactionRequest: From<TransactionRequest>>,
            Error: FromEvmError<Self::Evm>
                       + From<<Self::RpcConvert as RpcConvert>::Error>
                       + From<ProviderError>,
        > + SpawnBlocking,
    Self::Error: From<EthApiError>,
    N: HlNodeCore,
{
    #[inline]
    fn call_gas_limit(&self) -> u64 {
        self.inner.eth_api.gas_cap()
    }

    #[inline]
    fn max_simulate_blocks(&self) -> u64 {
        self.inner.eth_api.max_simulate_blocks()
    }
}
