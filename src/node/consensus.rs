use crate::{
    hardforks::HlHardforks,
    node::HlNode,
    {HlBlock, HlBlockBody, HlPrimitives},
};
use reth::{
    api::FullNodeTypes,
    beacon_consensus::EthBeaconConsensus,
    builder::{components::ConsensusBuilder, BuilderContext},
    consensus::{Consensus, ConsensusError, FullConsensus, HeaderValidator},
    consensus_common::validation::{
        validate_against_parent_4844, validate_against_parent_eip1559_base_fee,
        validate_against_parent_hash_number, validate_against_parent_timestamp,
    },
};
use reth_chainspec::EthChainSpec;
use reth_primitives::{Receipt, RecoveredBlock, SealedBlock, SealedHeader};
use reth_provider::BlockExecutionResult;
use std::sync::Arc;

/// A basic Hl consensus builder.
#[derive(Debug, Default, Clone, Copy)]
#[non_exhaustive]
pub struct HlConsensusBuilder;

impl<Node> ConsensusBuilder<Node> for HlConsensusBuilder
where
    Node: FullNodeTypes<Types = HlNode>,
{
    type Consensus = Arc<dyn FullConsensus<HlPrimitives, Error = ConsensusError>>;

    async fn build_consensus(self, ctx: &BuilderContext<Node>) -> eyre::Result<Self::Consensus> {
        Ok(Arc::new(HlConsensus::new(ctx.chain_spec())))
    }
}

/// HL consensus implementation.
///
/// Provides basic checks as outlined in the execution specs.
#[derive(Debug, Clone)]
pub struct HlConsensus<ChainSpec> {
    inner: EthBeaconConsensus<ChainSpec>,
    chain_spec: Arc<ChainSpec>,
}

impl<ChainSpec: EthChainSpec + HlHardforks> HlConsensus<ChainSpec> {
    /// Create a new instance of [`HlConsensus`]
    pub fn new(chain_spec: Arc<ChainSpec>) -> Self {
        Self {
            inner: EthBeaconConsensus::new(chain_spec.clone()),
            chain_spec,
        }
    }
}

impl<ChainSpec: EthChainSpec + HlHardforks> HeaderValidator for HlConsensus<ChainSpec> {
    fn validate_header(&self, _header: &SealedHeader) -> Result<(), ConsensusError> {
        // TODO: doesn't work because of extradata check
        // self.inner.validate_header(header)

        Ok(())
    }

    fn validate_header_against_parent(
        &self,
        header: &SealedHeader,
        parent: &SealedHeader,
    ) -> Result<(), ConsensusError> {
        validate_against_parent_hash_number(header.header(), parent)?;

        validate_against_parent_timestamp(header.header(), parent.header())?;

        validate_against_parent_eip1559_base_fee(
            header.header(),
            parent.header(),
            &self.chain_spec,
        )?;

        // ensure that the blob gas fields for this block
        if let Some(blob_params) = self.chain_spec.blob_params_at_timestamp(header.timestamp) {
            validate_against_parent_4844(header.header(), parent.header(), blob_params)?;
        }

        Ok(())
    }
}

impl<ChainSpec: EthChainSpec + HlHardforks> Consensus<HlBlock> for HlConsensus<ChainSpec> {
    type Error = ConsensusError;

    fn validate_body_against_header(
        &self,
        body: &HlBlockBody,
        header: &SealedHeader,
    ) -> Result<(), ConsensusError> {
        Consensus::<HlBlock>::validate_body_against_header(&self.inner, body, header)
    }

    fn validate_block_pre_execution(
        &self,
        _block: &SealedBlock<HlBlock>,
    ) -> Result<(), ConsensusError> {
        // Check ommers hash
        // let ommers_hash = block.body().calculate_ommers_root();
        // if Some(block.ommers_hash()) != ommers_hash {
        //     return Err(ConsensusError::BodyOmmersHashDiff(
        //         GotExpected {
        //             got: ommers_hash.unwrap_or(EMPTY_OMMER_ROOT_HASH),
        //             expected: block.ommers_hash(),
        //         }
        //         .into(),
        //     ))
        // }

        // // Check transaction root
        // if let Err(error) = block.ensure_transaction_root_valid() {
        //     return Err(ConsensusError::BodyTransactionRootDiff(error.into()))
        // }

        // if self.chain_spec.is_cancun_active_at_timestamp(block.timestamp()) {
        //     validate_cancun_gas(block)?;
        // } else {
        //     return Ok(())
        // }

        Ok(())
    }
}

impl<ChainSpec: EthChainSpec + HlHardforks> FullConsensus<HlPrimitives> for HlConsensus<ChainSpec> {
    fn validate_block_post_execution(
        &self,
        block: &RecoveredBlock<HlBlock>,
        result: &BlockExecutionResult<Receipt>,
    ) -> Result<(), ConsensusError> {
        FullConsensus::<HlPrimitives>::validate_block_post_execution(&self.inner, block, result)
    }
}
