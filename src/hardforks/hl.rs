#![allow(unused)]
use alloy_chains::{Chain, NamedChain};
use core::any::Any;
use reth_chainspec::ForkCondition;
use reth_ethereum_forks::{hardfork, ChainHardforks, EthereumHardfork, Hardfork};

hardfork!(
    /// The name of a hl hardfork.
    ///
    /// When building a list of hardforks for a chain, it's still expected to mix with [`EthereumHardfork`].
    /// There is no name for these hardforks; just some bugfixes on the evm chain.
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    HlHardfork {
        /// Initial version
        V1,
        /// block.number bugfix
        V2,
        /// gas mismatch bugfix
        V3,
    }
);

impl HlHardfork {
    /// Retrieves the activation block for the specified hardfork on the given chain.
    pub fn activation_block<H: Hardfork>(self, fork: H, chain: Chain) -> Option<u64> {
        if chain == Chain::from_named(NamedChain::Hyperliquid) {
            return Self::hl_mainnet_activation_block(fork);
        }

        None
    }

    /// Retrieves the activation timestamp for the specified hardfork on the given chain.
    pub fn activation_timestamp<H: Hardfork>(self, fork: H, chain: Chain) -> Option<u64> {
        None
    }

    /// Retrieves the activation block for the specified hardfork on the HyperLiquid mainnet.
    pub fn hl_mainnet_activation_block<H: Hardfork>(fork: H) -> Option<u64> {
        match_hardfork(
            fork,
            |fork| match fork {
                EthereumHardfork::Frontier
                | EthereumHardfork::Homestead
                | EthereumHardfork::Tangerine
                | EthereumHardfork::SpuriousDragon
                | EthereumHardfork::Byzantium
                | EthereumHardfork::Constantinople
                | EthereumHardfork::Petersburg
                | EthereumHardfork::Istanbul
                | EthereumHardfork::MuirGlacier
                | EthereumHardfork::Berlin
                | EthereumHardfork::London
                | EthereumHardfork::Shanghai
                | EthereumHardfork::Cancun => Some(0),
                _ => None,
            },
            |fork| match fork {
                Self::V1 | Self::V2 | Self::V3 => Some(0),
                _ => None,
            },
        )
    }

    /// Hl mainnet list of hardforks.
    pub fn hl_mainnet() -> ChainHardforks {
        ChainHardforks::new(vec![
            (EthereumHardfork::Frontier.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::Homestead.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::Tangerine.boxed(), ForkCondition::Block(0)),
            (
                EthereumHardfork::SpuriousDragon.boxed(),
                ForkCondition::Block(0),
            ),
            (EthereumHardfork::Byzantium.boxed(), ForkCondition::Block(0)),
            (
                EthereumHardfork::Constantinople.boxed(),
                ForkCondition::Block(0),
            ),
            (
                EthereumHardfork::Petersburg.boxed(),
                ForkCondition::Block(0),
            ),
            (EthereumHardfork::Istanbul.boxed(), ForkCondition::Block(0)),
            (
                EthereumHardfork::MuirGlacier.boxed(),
                ForkCondition::Block(0),
            ),
            (EthereumHardfork::Berlin.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::London.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::Shanghai.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::Cancun.boxed(), ForkCondition::Block(0)),
            (Self::V1.boxed(), ForkCondition::Block(0)),
            (Self::V2.boxed(), ForkCondition::Block(0)),
            (Self::V3.boxed(), ForkCondition::Block(0)),
        ])
    }
}

/// Match helper method since it's not possible to match on `dyn Hardfork`
fn match_hardfork<H, HF, HHF>(fork: H, hardfork_fn: HF, hl_hardfork_fn: HHF) -> Option<u64>
where
    H: Hardfork,
    HF: Fn(&EthereumHardfork) -> Option<u64>,
    HHF: Fn(&HlHardfork) -> Option<u64>,
{
    let fork: &dyn Any = &fork;
    if let Some(fork) = fork.downcast_ref::<EthereumHardfork>() {
        return hardfork_fn(fork);
    }
    fork.downcast_ref::<HlHardfork>().and_then(hl_hardfork_fn)
}
