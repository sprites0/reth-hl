use crate::{hardforks::HlHardforks, node::HlNode, HlBlock, HlBlockBody, HlPrimitives};
use alloy_consensus::BlockHeader as _;
use alloy_eips::eip7685::Requests;
use reth::{
    api::FullNodeTypes,
    beacon_consensus::EthBeaconConsensus,
    builder::{components::ConsensusBuilder, BuilderContext},
    consensus::{Consensus, ConsensusError, FullConsensus, HeaderValidator},
    consensus_common::validation::{
        validate_against_parent_4844, validate_against_parent_hash_number,
    },
};
use reth_chainspec::{EthChainSpec, EthereumHardforks};
use reth_primitives::{
    gas_spent_by_transactions, GotExpected, Receipt, RecoveredBlock, SealedBlock, SealedHeader,
};
use reth_primitives_traits::{Block, BlockHeader, Receipt as ReceiptTrait};
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
        Self { inner: EthBeaconConsensus::new(chain_spec.clone()), chain_spec }
    }
}

/// Validates the timestamp against the parent to make sure it is in the past.
#[inline]
pub fn validate_against_parent_timestamp<H: BlockHeader>(
    header: &H,
    parent: &H,
) -> Result<(), ConsensusError> {
    // NOTE: HyperEVM allows the timestamp to be the same as the parent (big and small blocks)
    if header.timestamp() < parent.timestamp() {
        return Err(ConsensusError::TimestampIsInPast {
            parent_timestamp: parent.timestamp(),
            timestamp: header.timestamp(),
        });
    }
    Ok(())
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

        // validate_against_parent_eip1559_base_fee(
        //     header.header(),
        //     parent.header(),
        //     &self.chain_spec,
        // )?;

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

mod reth_copy;

pub fn validate_block_post_execution<B, R, ChainSpec>(
    block: &RecoveredBlock<B>,
    chain_spec: &ChainSpec,
    receipts: &[R],
    requests: &Requests,
) -> Result<(), ConsensusError>
where
    B: Block,
    R: ReceiptTrait,
    ChainSpec: EthereumHardforks,
{
    use reth_copy::verify_receipts;
    // Copy of reth's validate_block_post_execution
    // Differences:
    // - Filter out system transactions for receipts check

    // Check if gas used matches the value set in header.
    let cumulative_gas_used =
        receipts.last().map(|receipt| receipt.cumulative_gas_used()).unwrap_or(0);
    if block.header().gas_used() != cumulative_gas_used {
        return Err(ConsensusError::BlockGasUsed {
            gas: GotExpected { got: cumulative_gas_used, expected: block.header().gas_used() },
            gas_spent_by_tx: gas_spent_by_transactions(receipts),
        });
    }

    // Before Byzantium, receipts contained state root that would mean that expensive
    // operation as hashing that is required for state root got calculated in every
    // transaction This was replaced with is_success flag.
    // See more about EIP here: https://eips.ethereum.org/EIPS/eip-658
    if chain_spec.is_byzantium_active_at_block(block.header().number()) {
        let receipts_for_root =
            receipts.iter().filter(|&r| r.cumulative_gas_used() != 0).cloned().collect::<Vec<_>>();
        if let Err(error) = verify_receipts(
            block.header().receipts_root(),
            block.header().logs_bloom(),
            &receipts_for_root,
        ) {
            tracing::debug!(%error, ?receipts, "receipts verification failed");
            return Err(error);
        }
    }

    // Validate that the header requests hash matches the calculated requests hash
    if chain_spec.is_prague_active_at_timestamp(block.header().timestamp()) {
        let Some(header_requests_hash) = block.header().requests_hash() else {
            return Err(ConsensusError::RequestsHashMissing);
        };
        let requests_hash = requests.requests_hash();
        if requests_hash != header_requests_hash {
            return Err(ConsensusError::BodyRequestsHashDiff(
                GotExpected::new(requests_hash, header_requests_hash).into(),
            ));
        }
    }

    Ok(())
}

impl<ChainSpec: EthChainSpec + HlHardforks> FullConsensus<HlPrimitives> for HlConsensus<ChainSpec> {
    fn validate_block_post_execution(
        &self,
        block: &RecoveredBlock<HlBlock>,
        result: &BlockExecutionResult<Receipt>,
    ) -> Result<(), ConsensusError> {
        validate_block_post_execution(block, &self.chain_spec, &result.receipts, &result.requests)
    }
}
