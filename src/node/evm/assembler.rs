use crate::{
    node::evm::config::{HlBlockExecutorFactory, HlEvmConfig},
    HlBlock, HlBlockBody,
};
use alloy_consensus::{Block, Header};
use reth_evm::{
    block::BlockExecutionError,
    execute::{BlockAssembler, BlockAssemblerInput},
};

impl BlockAssembler<HlBlockExecutorFactory> for HlEvmConfig {
    type Block = HlBlock;

    fn assemble_block(
        &self,
        input: BlockAssemblerInput<'_, '_, HlBlockExecutorFactory, Header>,
    ) -> Result<Self::Block, BlockExecutionError> {
        let Block { header, body: inner } = self.block_assembler.assemble_block(input)?;
        Ok(HlBlock {
            header,
            body: HlBlockBody {
                inner,
                // HACK: we're setting sidecars to `None` here but ideally we should somehow get
                // them from the payload builder.
                //
                // Payload building is out of scope of reth-bsc for now, so this is not critical
                sidecars: None,
                read_precompile_calls: None,
            },
        })
    }
}
