use super::hl::HL_MAINNET;
use reth_chainspec::ChainSpec;
use reth_cli::chainspec::ChainSpecParser;
use std::sync::Arc;

/// Chains supported by HyperEVM. First value should be used as the default.
pub const SUPPORTED_CHAINS: &[&str] = &["mainnet"];

/// Hyperliquid chain specification parser.
#[derive(Debug, Clone, Default)]
pub struct HlChainSpecParser;

impl ChainSpecParser for HlChainSpecParser {
    type ChainSpec = ChainSpec;

    const SUPPORTED_CHAINS: &'static [&'static str] = SUPPORTED_CHAINS;

    fn parse(s: &str) -> eyre::Result<Arc<ChainSpec>> {
        chain_value_parser(s)
    }
}

/// Clap value parser for [`ChainSpec`]s.
///
/// Currently only mainnet is supported.
pub fn chain_value_parser(s: &str) -> eyre::Result<Arc<ChainSpec>> {
    match s {
        "mainnet" => Ok(HL_MAINNET.clone().into()),
        _ => Err(eyre::eyre!("Unsupported chain: {}", s)),
    }
}
