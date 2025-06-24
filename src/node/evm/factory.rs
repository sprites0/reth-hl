use super::HlEvm;
use crate::evm::{
    api::{
        builder::HlBuilder,
        ctx::{DefaultHl, HlContext},
    },
    spec::HlSpecId,
    transaction::HlTxEnv,
};
use reth_evm::{precompiles::PrecompilesMap, EvmEnv, EvmFactory};
use reth_revm::{Context, Database};
use revm::{
    context::{
        result::{EVMError, HaltReason},
        TxEnv,
    },
    handler::EthPrecompiles,
    inspector::NoOpInspector,
    Inspector,
};

/// Factory producing [`HlEvm`].
#[derive(Debug, Default, Clone, Copy)]
#[non_exhaustive]
pub struct HlEvmFactory;

impl EvmFactory for HlEvmFactory {
    type Evm<DB: Database<Error: Send + Sync + 'static>, I: Inspector<HlContext<DB>>> =
        HlEvm<DB, I, Self::Precompiles>;
    type Context<DB: Database<Error: Send + Sync + 'static>> = HlContext<DB>;
    type Tx = HlTxEnv<TxEnv>;
    type Error<DBError: core::error::Error + Send + Sync + 'static> = EVMError<DBError>;
    type HaltReason = HaltReason;
    type Spec = HlSpecId;
    type Precompiles = PrecompilesMap;

    fn create_evm<DB: Database<Error: Send + Sync + 'static>>(
        &self,
        db: DB,
        input: EvmEnv<HlSpecId>,
    ) -> Self::Evm<DB, NoOpInspector> {
        HlEvm {
            inner: Context::hl()
                .with_block(input.block_env)
                .with_cfg(input.cfg_env)
                .with_db(db)
                .build_hl_with_inspector(NoOpInspector {})
                .with_precompiles(PrecompilesMap::from_static(
                    EthPrecompiles::default().precompiles,
                )),
            inspect: false,
        }
    }

    fn create_evm_with_inspector<
        DB: Database<Error: Send + Sync + 'static>,
        I: Inspector<Self::Context<DB>>,
    >(
        &self,
        db: DB,
        input: EvmEnv<HlSpecId>,
        inspector: I,
    ) -> Self::Evm<DB, I> {
        HlEvm {
            inner: Context::hl()
                .with_block(input.block_env)
                .with_cfg(input.cfg_env)
                .with_db(db)
                .build_hl_with_inspector(inspector)
                .with_precompiles(PrecompilesMap::from_static(
                    EthPrecompiles::default().precompiles,
                )),
            inspect: true,
        }
    }
}
