use super::HlEthApi;
use reth::rpc::server_types::eth::EthApiError;
use reth_evm::TxEnvFor;
use reth_rpc_eth_api::{
    helpers::{estimate::EstimateCall, Call, EthCall},
    FromEvmError, RpcConvert, RpcNodeCore,
};

impl<N, Rpc> EthCall for HlEthApi<N, Rpc>
where
    N: RpcNodeCore,
    EthApiError: FromEvmError<N::Evm>,
    Rpc: RpcConvert<Primitives = N::Primitives, Error = EthApiError, TxEnv = TxEnvFor<N::Evm>>,
{
}

impl<N, Rpc> EstimateCall for HlEthApi<N, Rpc>
where
    N: RpcNodeCore,
    EthApiError: FromEvmError<N::Evm>,
    Rpc: RpcConvert<Primitives = N::Primitives, Error = EthApiError, TxEnv = TxEnvFor<N::Evm>>,
{
}

impl<N, Rpc> Call for HlEthApi<N, Rpc>
where
    N: RpcNodeCore,
    EthApiError: FromEvmError<N::Evm>,
    Rpc: RpcConvert<Primitives = N::Primitives, Error = EthApiError, TxEnv = TxEnvFor<N::Evm>>,
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
