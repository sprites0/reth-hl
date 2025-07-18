use alloy_network::Ethereum;
use alloy_primitives::U256;
use reth::{
    builder::{
        rpc::{EthApiBuilder, EthApiCtx},
        FullNodeComponents,
    },
    chainspec::EthChainSpec,
    primitives::EthereumHardforks,
    providers::ChainSpecProvider,
    rpc::{
        eth::{core::EthApiInner, DevSigner, FullEthApiServer},
        server_types::eth::{EthApiError, EthStateCache, FeeHistoryCache, GasPriceOracle},
    },
    tasks::{
        pool::{BlockingTaskGuard, BlockingTaskPool},
        TaskSpawner,
    },
    transaction_pool::TransactionPool,
};
use reth_evm::ConfigureEvm;
use reth_network::NetworkInfo;
use reth_primitives::NodePrimitives;
use reth_provider::{
    BlockNumReader, BlockReader, BlockReaderIdExt, ProviderBlock, ProviderHeader, ProviderReceipt,
    ProviderTx, StageCheckpointReader, StateProviderFactory,
};
use reth_rpc_eth_api::{
    helpers::{
        AddDevSigners, EthApiSpec, EthFees, EthSigner, EthState, LoadBlock, LoadFee, LoadState,
        SpawnBlocking, Trace,
    },
    EthApiTypes, FromEvmError, RpcConverter, RpcNodeCore, RpcNodeCoreExt,
};
use std::{fmt, sync::Arc};

mod block;
mod call;
pub mod engine_api;
mod transaction;

/// A helper trait with requirements for [`RpcNodeCore`] to be used in [`HlEthApi`].
pub trait HlNodeCore: RpcNodeCore<Provider: BlockReader> {}
impl<T> HlNodeCore for T where T: RpcNodeCore<Provider: BlockReader> {}

/// Adapter for [`EthApiInner`], which holds all the data required to serve core `eth_` API.
pub type EthApiNodeBackend<N> = EthApiInner<
    <N as RpcNodeCore>::Provider,
    <N as RpcNodeCore>::Pool,
    <N as RpcNodeCore>::Network,
    <N as RpcNodeCore>::Evm,
>;

/// Container type `HlEthApi`
#[allow(missing_debug_implementations)]
pub(crate) struct HlEthApiInner<N: HlNodeCore> {
    /// Gateway to node's core components.
    pub(crate) eth_api: EthApiNodeBackend<N>,
}

#[derive(Clone)]
pub struct HlEthApi<N: HlNodeCore> {
    /// Gateway to node's core components.
    pub(crate) inner: Arc<HlEthApiInner<N>>,
    /// Converter for RPC types.
    tx_resp_builder: RpcConverter<Ethereum, N::Evm, EthApiError, ()>,
    /// Whether the node is in HL node compliant mode.
    pub(crate) hl_node_compliant: bool,
}

impl<N: HlNodeCore> fmt::Debug for HlEthApi<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HlEthApi").finish_non_exhaustive()
    }
}

impl<N> EthApiTypes for HlEthApi<N>
where
    Self: Send + Sync,
    N: HlNodeCore,
    N::Evm: std::fmt::Debug,
{
    type Error = EthApiError;
    type NetworkTypes = Ethereum;
    type RpcConvert = RpcConverter<Ethereum, N::Evm, EthApiError, ()>;

    fn tx_resp_builder(&self) -> &Self::RpcConvert {
        &self.tx_resp_builder
    }
}

impl<N> RpcNodeCore for HlEthApi<N>
where
    N: HlNodeCore,
{
    type Primitives = N::Primitives;
    type Provider = N::Provider;
    type Pool = N::Pool;
    type Evm = <N as RpcNodeCore>::Evm;
    type Network = <N as RpcNodeCore>::Network;
    type PayloadBuilder = ();

    #[inline]
    fn pool(&self) -> &Self::Pool {
        self.inner.eth_api.pool()
    }

    #[inline]
    fn evm_config(&self) -> &Self::Evm {
        self.inner.eth_api.evm_config()
    }

    #[inline]
    fn network(&self) -> &Self::Network {
        self.inner.eth_api.network()
    }

    #[inline]
    fn payload_builder(&self) -> &Self::PayloadBuilder {
        &()
    }

    #[inline]
    fn provider(&self) -> &Self::Provider {
        self.inner.eth_api.provider()
    }
}

impl<N> RpcNodeCoreExt for HlEthApi<N>
where
    N: HlNodeCore,
{
    #[inline]
    fn cache(&self) -> &EthStateCache<ProviderBlock<N::Provider>, ProviderReceipt<N::Provider>> {
        self.inner.eth_api.cache()
    }
}

impl<N> EthApiSpec for HlEthApi<N>
where
    N: HlNodeCore<
        Provider: ChainSpecProvider<ChainSpec: EthereumHardforks>
                      + BlockNumReader
                      + StageCheckpointReader,
        Network: NetworkInfo,
    >,
{
    type Transaction = ProviderTx<Self::Provider>;

    #[inline]
    fn starting_block(&self) -> U256 {
        self.inner.eth_api.starting_block()
    }

    #[inline]
    fn signers(&self) -> &parking_lot::RwLock<Vec<Box<dyn EthSigner<ProviderTx<Self::Provider>>>>> {
        self.inner.eth_api.signers()
    }
}

impl<N> SpawnBlocking for HlEthApi<N>
where
    Self: Send + Sync + Clone + 'static,
    N: HlNodeCore,
    N::Evm: std::fmt::Debug,
{
    #[inline]
    fn io_task_spawner(&self) -> impl TaskSpawner {
        self.inner.eth_api.task_spawner()
    }

    #[inline]
    fn tracing_task_pool(&self) -> &BlockingTaskPool {
        self.inner.eth_api.blocking_task_pool()
    }

    #[inline]
    fn tracing_task_guard(&self) -> &BlockingTaskGuard {
        self.inner.eth_api.blocking_task_guard()
    }
}

impl<N> LoadFee for HlEthApi<N>
where
    Self: LoadBlock<Provider = N::Provider>,
    N: HlNodeCore<
        Provider: BlockReaderIdExt
                      + ChainSpecProvider<ChainSpec: EthChainSpec + EthereumHardforks>
                      + StateProviderFactory,
    >,
{
    #[inline]
    fn gas_oracle(&self) -> &GasPriceOracle<Self::Provider> {
        self.inner.eth_api.gas_oracle()
    }

    #[inline]
    fn fee_history_cache(&self) -> &FeeHistoryCache {
        self.inner.eth_api.fee_history_cache()
    }
}

impl<N> LoadState for HlEthApi<N>
where
    N: HlNodeCore<
        Provider: StateProviderFactory + ChainSpecProvider<ChainSpec: EthereumHardforks>,
        Pool: TransactionPool,
    >,
    N::Evm: std::fmt::Debug,
{
}

impl<N> EthState for HlEthApi<N>
where
    Self: LoadState + SpawnBlocking,
    N: HlNodeCore,
{
    #[inline]
    fn max_proof_window(&self) -> u64 {
        self.inner.eth_api.eth_proof_window()
    }
}

impl<N> EthFees for HlEthApi<N>
where
    Self: LoadFee<
        Provider: ChainSpecProvider<
            ChainSpec: EthChainSpec<Header = ProviderHeader<Self::Provider>>,
        >,
    >,
    N: HlNodeCore,
{
}

impl<N> Trace for HlEthApi<N>
where
    Self: RpcNodeCore<Provider: BlockReader>
        + LoadState<
            Evm: ConfigureEvm<
                Primitives: NodePrimitives<
                    BlockHeader = ProviderHeader<Self::Provider>,
                    SignedTx = ProviderTx<Self::Provider>,
                >,
            >,
            Error: FromEvmError<Self::Evm>,
        >,
    N: HlNodeCore,
{
}

impl<N> AddDevSigners for HlEthApi<N>
where
    N: HlNodeCore,
{
    fn with_dev_accounts(&self) {
        *self.inner.eth_api.signers().write() = DevSigner::random_signers(20)
    }
}

/// Builds [`HlEthApi`] for HL.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct HlEthApiBuilder {
    /// Whether the node is in HL node compliant mode.
    pub(crate) hl_node_compliant: bool,
}

impl<N> EthApiBuilder<N> for HlEthApiBuilder
where
    N: FullNodeComponents,
    HlEthApi<N>: FullEthApiServer<Provider = N::Provider, Pool = N::Pool>,
{
    type EthApi = HlEthApi<N>;

    async fn build_eth_api(self, ctx: EthApiCtx<'_, N>) -> eyre::Result<Self::EthApi> {
        let eth_api = reth::rpc::eth::EthApiBuilder::new(
            ctx.components.provider().clone(),
            ctx.components.pool().clone(),
            ctx.components.network().clone(),
            ctx.components.evm_config().clone(),
        )
        .eth_cache(ctx.cache)
        .task_spawner(ctx.components.task_executor().clone())
        .gas_cap(ctx.config.rpc_gas_cap.into())
        .max_simulate_blocks(ctx.config.rpc_max_simulate_blocks)
        .eth_proof_window(ctx.config.eth_proof_window)
        .fee_history_cache_config(ctx.config.fee_history_cache)
        .proof_permits(ctx.config.proof_permits)
        .build_inner();

        Ok(HlEthApi {
            inner: Arc::new(HlEthApiInner { eth_api }),
            tx_resp_builder: Default::default(),
            hl_node_compliant: self.hl_node_compliant,
        })
    }
}
