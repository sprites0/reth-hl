//! Chain specification for HyperEVM.
pub mod hl;
pub mod parser;

use crate::hardforks::{hl::HlHardfork, HlHardforks};
use alloy_consensus::Header;
use alloy_eips::eip7840::BlobParams;
use alloy_genesis::Genesis;
use alloy_primitives::{Address, B256, U256};
use reth_chainspec::{
    BaseFeeParams, ChainSpec, DepositContract, EthChainSpec, EthereumHardfork, EthereumHardforks,
    ForkCondition, ForkFilter, ForkId, Hardforks, Head,
};
use reth_discv4::NodeRecord;
use reth_evm::eth::spec::EthExecutorSpec;
use std::{fmt::Display, sync::Arc};

/// Hl chain spec type.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HlChainSpec {
    /// [`ChainSpec`].
    pub inner: ChainSpec,
}

impl EthChainSpec for HlChainSpec {
    type Header = Header;

    fn blob_params_at_timestamp(&self, timestamp: u64) -> Option<BlobParams> {
        self.inner.blob_params_at_timestamp(timestamp)
    }

    fn final_paris_total_difficulty(&self) -> Option<U256> {
        self.inner.final_paris_total_difficulty()
    }

    fn chain(&self) -> alloy_chains::Chain {
        self.inner.chain()
    }

    fn base_fee_params_at_block(&self, block_number: u64) -> BaseFeeParams {
        self.inner.base_fee_params_at_block(block_number)
    }

    fn base_fee_params_at_timestamp(&self, timestamp: u64) -> BaseFeeParams {
        self.inner.base_fee_params_at_timestamp(timestamp)
    }

    fn deposit_contract(&self) -> Option<&DepositContract> {
        None
    }

    fn genesis_hash(&self) -> B256 {
        self.inner.genesis_hash()
    }

    fn prune_delete_limit(&self) -> usize {
        self.inner.prune_delete_limit()
    }

    fn display_hardforks(&self) -> Box<dyn Display> {
        Box::new(self.inner.display_hardforks())
    }

    fn genesis_header(&self) -> &Header {
        self.inner.genesis_header()
    }

    fn genesis(&self) -> &Genesis {
        self.inner.genesis()
    }

    fn bootnodes(&self) -> Option<Vec<NodeRecord>> {
        self.inner.bootnodes()
    }

    fn is_optimism(&self) -> bool {
        false
    }
}

impl Hardforks for HlChainSpec {
    fn fork<H: reth_chainspec::Hardfork>(&self, fork: H) -> reth_chainspec::ForkCondition {
        self.inner.fork(fork)
    }

    fn forks_iter(
        &self,
    ) -> impl Iterator<Item = (&dyn reth_chainspec::Hardfork, reth_chainspec::ForkCondition)> {
        self.inner.forks_iter()
    }

    fn fork_id(&self, head: &Head) -> ForkId {
        self.inner.fork_id(head)
    }

    fn latest_fork_id(&self) -> ForkId {
        self.inner.latest_fork_id()
    }

    fn fork_filter(&self, head: Head) -> ForkFilter {
        self.inner.fork_filter(head)
    }
}

impl From<ChainSpec> for HlChainSpec {
    fn from(value: ChainSpec) -> Self {
        Self { inner: value }
    }
}

impl EthereumHardforks for HlChainSpec {
    fn ethereum_fork_activation(&self, fork: EthereumHardfork) -> ForkCondition {
        self.inner.ethereum_fork_activation(fork)
    }
}

impl HlHardforks for HlChainSpec {
    fn hl_fork_activation(&self, fork: HlHardfork) -> ForkCondition {
        self.fork(fork)
    }
}

impl EthExecutorSpec for HlChainSpec {
    fn deposit_contract_address(&self) -> Option<Address> {
        None
    }
}

impl From<HlChainSpec> for ChainSpec {
    fn from(value: HlChainSpec) -> Self {
        value.inner
    }
}

impl HlHardforks for Arc<HlChainSpec> {
    fn hl_fork_activation(&self, fork: HlHardfork) -> ForkCondition {
        self.as_ref().hl_fork_activation(fork)
    }
}

impl HlChainSpec {
    pub const MAINNET_RPC_URL: &str = "https://rpc.hyperliquid.xyz/evm";
    pub const TESTNET_RPC_URL: &str = "https://rpc.hyperliquid-testnet.xyz/evm";

    pub fn official_rpc_url(&self) -> &'static str {
        match self.inner.chain().id() {
            999 => Self::MAINNET_RPC_URL,
            998 => Self::TESTNET_RPC_URL,
            _ => unreachable!("Unreachable since ChainSpecParser won't return other chains"),
        }
    }
}
