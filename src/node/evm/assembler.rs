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
        let HlBlock { header, body } = self.block_assembler.assemble_block(input)?;
        Ok(HlBlock { header, body })
    }
}
