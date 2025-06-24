use crate::{
    evm::{
        api::{ctx::HlContext, HlEvmInner},
        spec::HlSpecId,
        transaction::HlTxEnv,
    },
    node::HlNode,
};
use alloy_primitives::{Address, Bytes};
use config::HlEvmConfig;
use reth::{
    api::FullNodeTypes,
    builder::{components::ExecutorBuilder, BuilderContext},
};
use reth_evm::{Evm, EvmEnv};
use revm::{
    context::{
        result::{EVMError, HaltReason, ResultAndState},
        BlockEnv, TxEnv,
    },
    handler::{instructions::EthInstructions, EthPrecompiles, PrecompileProvider},
    interpreter::{interpreter::EthInterpreter, InterpreterResult},
    Context, Database, ExecuteEvm, InspectEvm, Inspector,
};
use std::ops::{Deref, DerefMut};

mod assembler;
pub mod config;
mod executor;
mod factory;
mod patch;

/// HL EVM implementation.
///
/// This is a wrapper type around the `revm` evm with optional [`Inspector`] (tracing)
/// support. [`Inspector`] support is configurable at runtime because it's part of the underlying
#[allow(missing_debug_implementations)]
pub struct HlEvm<DB: Database, I, P = EthPrecompiles> {
    pub inner: HlEvmInner<HlContext<DB>, I, EthInstructions<EthInterpreter, HlContext<DB>>, P>,
    pub inspect: bool,
}

impl<DB: Database, I, P> HlEvm<DB, I, P> {
    /// Provides a reference to the EVM context.
    pub const fn ctx(&self) -> &HlContext<DB> {
        &self.inner.0.ctx
    }

    /// Provides a mutable reference to the EVM context.
    pub fn ctx_mut(&mut self) -> &mut HlContext<DB> {
        &mut self.inner.0.ctx
    }
}

impl<DB: Database, I, P> Deref for HlEvm<DB, I, P> {
    type Target = HlContext<DB>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.ctx()
    }
}

impl<DB: Database, I, P> DerefMut for HlEvm<DB, I, P> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.ctx_mut()
    }
}

impl<DB, I, P> Evm for HlEvm<DB, I, P>
where
    DB: Database,
    I: Inspector<HlContext<DB>>,
    P: PrecompileProvider<HlContext<DB>, Output = InterpreterResult>,
    <DB as revm::Database>::Error: std::marker::Send + std::marker::Sync + 'static,
{
    type DB = DB;
    type Tx = HlTxEnv<TxEnv>;
    type Error = EVMError<DB::Error>;
    type HaltReason = HaltReason;
    type Spec = HlSpecId;
    type Precompiles = P;
    type Inspector = I;

    fn chain_id(&self) -> u64 {
        self.cfg.chain_id
    }

    fn block(&self) -> &BlockEnv {
        &self.block
    }

    fn transact_raw(
        &mut self,
        tx: Self::Tx,
    ) -> Result<ResultAndState<Self::HaltReason>, Self::Error> {
        if self.inspect {
            self.inner.set_tx(tx);
            self.inner.inspect_replay()
        } else {
            self.inner.transact(tx)
        }
    }

    fn transact_system_call(
        &mut self,
        _caller: Address,
        _contract: Address,
        _data: Bytes,
    ) -> Result<ResultAndState<Self::HaltReason>, Self::Error> {
        unimplemented!()
    }

    fn db_mut(&mut self) -> &mut Self::DB {
        &mut self.journaled_state.database
    }

    fn finish(self) -> (Self::DB, EvmEnv<Self::Spec>) {
        let Context {
            block: block_env,
            cfg: cfg_env,
            journaled_state,
            ..
        } = self.inner.0.ctx;

        (journaled_state.database, EvmEnv { block_env, cfg_env })
    }

    fn set_inspector_enabled(&mut self, enabled: bool) {
        self.inspect = enabled;
    }

    fn precompiles_mut(&mut self) -> &mut Self::Precompiles {
        &mut self.inner.0.precompiles
    }

    fn inspector_mut(&mut self) -> &mut Self::Inspector {
        &mut self.inner.0.inspector
    }

    fn precompiles(&self) -> &Self::Precompiles {
        &self.inner.0.precompiles
    }

    fn inspector(&self) -> &Self::Inspector {
        &self.inner.0.inspector
    }
}

/// A regular hl evm and executor builder.
#[derive(Debug, Default, Clone, Copy)]
#[non_exhaustive]
pub struct HlExecutorBuilder;

impl<Node> ExecutorBuilder<Node> for HlExecutorBuilder
where
    Node: FullNodeTypes<Types = HlNode>,
{
    type EVM = HlEvmConfig;

    async fn build_evm(self, ctx: &BuilderContext<Node>) -> eyre::Result<Self::EVM> {
        let evm_config = HlEvmConfig::hl(ctx.chain_spec());
        Ok(evm_config)
    }
}
