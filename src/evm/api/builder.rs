use super::HlEvmInner;
use crate::evm::{spec::HlSpecId, transaction::HlTxTr};
use revm::{
    context::{Cfg, JournalOutput},
    context_interface::{Block, JournalTr},
    handler::instructions::EthInstructions,
    interpreter::interpreter::EthInterpreter,
    Context, Database,
};

/// Trait that allows for hl HlEvm to be built.
pub trait HlBuilder: Sized {
    /// Type of the context.
    type Context;

    /// Build the hl with an inspector.
    fn build_hl_with_inspector<INSP>(
        self,
        inspector: INSP,
    ) -> HlEvmInner<Self::Context, INSP, EthInstructions<EthInterpreter, Self::Context>>;
}

impl<BLOCK, TX, CFG, DB, JOURNAL> HlBuilder for Context<BLOCK, TX, CFG, DB, JOURNAL>
where
    BLOCK: Block,
    TX: HlTxTr,
    CFG: Cfg<Spec = HlSpecId>,
    DB: Database,
    JOURNAL: JournalTr<Database = DB, FinalOutput = JournalOutput>,
{
    type Context = Self;

    fn build_hl_with_inspector<INSP>(
        self,
        inspector: INSP,
    ) -> HlEvmInner<Self::Context, INSP, EthInstructions<EthInterpreter, Self::Context>> {
        HlEvmInner::new(self, inspector)
    }
}
