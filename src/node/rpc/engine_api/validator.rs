use crate::{
    chainspec::HlChainSpec,
    hardforks::HlHardforks,
    node::{HlBlock, HlPrimitives},
};
use alloy_consensus::BlockHeader;
use alloy_eips::eip4895::Withdrawal;
use alloy_primitives::B256;
use alloy_rpc_types_engine::{PayloadAttributes, PayloadError};
use reth::{
    api::{FullNodeComponents, NodeTypes},
    builder::{rpc::EngineValidatorBuilder, AddOnsContext},
    consensus::ConsensusError,
};
use reth_engine_primitives::{EngineValidator, ExecutionPayload, PayloadValidator};
use reth_payload_primitives::{
    EngineApiMessageVersion, EngineObjectValidationError, NewPayloadError, PayloadOrAttributes,
    PayloadTypes,
};
use reth_primitives::{RecoveredBlock, SealedBlock};
use reth_primitives_traits::Block as _;
use reth_trie_common::HashedPostState;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::payload::HlPayloadTypes;

#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct HlEngineValidatorBuilder;

impl<Node, Types> EngineValidatorBuilder<Node> for HlEngineValidatorBuilder
where
    Types: NodeTypes<ChainSpec = HlChainSpec, Payload = HlPayloadTypes, Primitives = HlPrimitives>,
    Node: FullNodeComponents<Types = Types>,
{
    type Validator = HlEngineValidator;

    async fn build(self, ctx: &AddOnsContext<'_, Node>) -> eyre::Result<Self::Validator> {
        Ok(HlEngineValidator::new(Arc::new(ctx.config.chain.clone().as_ref().clone())))
    }
}

/// Validator for Optimism engine API.
#[derive(Debug, Clone)]
pub struct HlEngineValidator {
    inner: HlExecutionPayloadValidator<HlChainSpec>,
}

impl HlEngineValidator {
    /// Instantiates a new validator.
    pub fn new(chain_spec: Arc<HlChainSpec>) -> Self {
        Self { inner: HlExecutionPayloadValidator { inner: chain_spec } }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HlExecutionData(pub HlBlock);

impl ExecutionPayload for HlExecutionData {
    fn parent_hash(&self) -> B256 {
        self.0.header.parent_hash()
    }

    fn block_hash(&self) -> B256 {
        self.0.header.hash_slow()
    }

    fn block_number(&self) -> u64 {
        self.0.header.number()
    }

    fn withdrawals(&self) -> Option<&Vec<Withdrawal>> {
        None
    }

    fn parent_beacon_block_root(&self) -> Option<B256> {
        None
    }

    fn timestamp(&self) -> u64 {
        self.0.header.timestamp()
    }

    fn gas_used(&self) -> u64 {
        self.0.header.gas_used()
    }
}

impl PayloadValidator for HlEngineValidator {
    type Block = HlBlock;
    type ExecutionData = HlExecutionData;

    fn ensure_well_formed_payload(
        &self,
        payload: Self::ExecutionData,
    ) -> Result<RecoveredBlock<Self::Block>, NewPayloadError> {
        let sealed_block =
            self.inner.ensure_well_formed_payload(payload).map_err(NewPayloadError::other)?;
        sealed_block.try_recover().map_err(|e| NewPayloadError::Other(e.into()))
    }

    fn validate_block_post_execution_with_hashed_state(
        &self,
        _state_updates: &HashedPostState,
        _block: &RecoveredBlock<Self::Block>,
    ) -> Result<(), ConsensusError> {
        Ok(())
    }
}

impl<Types> EngineValidator<Types> for HlEngineValidator
where
    Types: PayloadTypes<PayloadAttributes = PayloadAttributes, ExecutionData = HlExecutionData>,
{
    fn validate_version_specific_fields(
        &self,
        _version: EngineApiMessageVersion,
        _payload_or_attrs: PayloadOrAttributes<'_, Self::ExecutionData, PayloadAttributes>,
    ) -> Result<(), EngineObjectValidationError> {
        Ok(())
    }

    fn ensure_well_formed_attributes(
        &self,
        _version: EngineApiMessageVersion,
        _attributes: &PayloadAttributes,
    ) -> Result<(), EngineObjectValidationError> {
        Ok(())
    }
}

/// Execution payload validator.
#[derive(Clone, Debug)]
pub struct HlExecutionPayloadValidator<ChainSpec> {
    /// Chain spec to validate against.
    #[allow(unused)]
    inner: Arc<ChainSpec>,
}

impl<ChainSpec> HlExecutionPayloadValidator<ChainSpec>
where
    ChainSpec: HlHardforks,
{
    pub fn ensure_well_formed_payload(
        &self,
        payload: HlExecutionData,
    ) -> Result<SealedBlock<HlBlock>, PayloadError> {
        let block = payload.0;

        let expected_hash = block.header.hash_slow();

        // First parse the block
        let sealed_block = block.seal_slow();

        // Ensure the hash included in the payload matches the block hash
        if expected_hash != sealed_block.hash() {
            return Err(PayloadError::BlockHash {
                execution: sealed_block.hash(),
                consensus: expected_hash,
            })?;
        }

        Ok(sealed_block)
    }
}
