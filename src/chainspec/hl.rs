use alloy_chains::{Chain, NamedChain};
use alloy_primitives::{b256, Address, Bytes, B256, B64, U256};
use std::sync::Arc;
use once_cell::sync::Lazy;
use reth_chainspec::{ChainSpec, DEV_HARDFORKS};
use reth_primitives::{Header, SealedHeader};

static GENESIS_HASH: B256 =
    b256!("d8fcc13b6a195b88b7b2da3722ff6cad767b13a8c1e9ffb1c73aa9d216d895f0");

/// The Hyperliqiud Mainnet spec
pub static HL_MAINNET: Lazy<Arc<ChainSpec>> = Lazy::new(|| {
    ChainSpec {
        chain: Chain::from_named(NamedChain::Hyperliquid),
        genesis: serde_json::from_str(include_str!("genesis.json"))
            .expect("Can't deserialize Hyperliquid Mainnet genesis json"),
        genesis_header: empty_genesis_header(),
        paris_block_and_final_difficulty: Some((0, U256::from(0))),
        hardforks: DEV_HARDFORKS.clone(),
        prune_delete_limit: 10000,
        ..Default::default()
    }
    .into()
});

/// Empty genesis header for Hyperliquid Mainnet.
///
/// The exact value is not known per se, but the parent hash of block 1 is known to be
/// [GENESIS_HASH].
fn empty_genesis_header() -> SealedHeader {
    SealedHeader::new(
        Header {
            parent_hash: B256::ZERO,
            number: 0,
            timestamp: 0,
            transactions_root: B256::ZERO,
            receipts_root: B256::ZERO,
            state_root: B256::ZERO,
            gas_used: 0,
            gas_limit: 0x1c9c380,
            difficulty: U256::ZERO,
            mix_hash: B256::ZERO,
            extra_data: Bytes::new(),
            nonce: B64::ZERO,
            ommers_hash: B256::ZERO,
            beneficiary: Address::ZERO,
            logs_bloom: Default::default(),
            base_fee_per_gas: Some(0),
            withdrawals_root: Some(B256::ZERO),
            blob_gas_used: Some(0),
            excess_blob_gas: Some(0),
            parent_beacon_block_root: Some(B256::ZERO),
            requests_hash: Some(B256::ZERO),
        },
        GENESIS_HASH,
    )
}
