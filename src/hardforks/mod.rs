//! Hard forks of HyperEVM.
#![allow(unused)]
pub mod hl;

use hl::HlHardfork;
use reth_chainspec::{EthereumHardforks, ForkCondition};
use std::sync::Arc;

/// Extends [`EthereumHardforks`] with hl helper methods.
/// 
/// Currently a placeholder for future use.
pub trait HlHardforks: EthereumHardforks {}

impl<T: HlHardforks> HlHardforks for Arc<T> {}
