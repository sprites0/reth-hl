//! Hard forks of hl protocol.
#![allow(unused)]
use hl::HlHardfork;
use reth_chainspec::{EthereumHardforks, ForkCondition};

pub mod hl;

/// Extends [`EthereumHardforks`] with hl helper methods.
pub trait HlHardforks: EthereumHardforks {
    /// Retrieves [`ForkCondition`] by an [`HlHardfork`]. If `fork` is not present, returns
    /// [`ForkCondition::Never`].
    fn hl_fork_activation(&self, fork: HlHardfork) -> ForkCondition;
}
