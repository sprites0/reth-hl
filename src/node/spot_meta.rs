use alloy_primitives::{Address, U256};
use eyre::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const MAINNET_CHAIN_ID: u64 = 999;
pub(crate) const TESTNET_CHAIN_ID: u64 = 998;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EvmContract {
    address: Address,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpotToken {
    index: u64,
    #[serde(rename = "evmContract")]
    evm_contract: Option<EvmContract>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotMeta {
    tokens: Vec<SpotToken>,
}

#[derive(Debug, Clone)]
pub(crate) struct SpotId {
    pub index: u64,
}

impl SpotId {
    pub(crate) fn to_s(&self) -> U256 {
        let mut addr = [0u8; 32];
        addr[12] = 0x20;
        addr[24..32].copy_from_slice(self.index.to_be_bytes().as_ref());
        U256::from_be_bytes(addr)
    }
}

async fn fetch_spot_meta(chain_id: u64) -> Result<SpotMeta> {
    let url = match chain_id {
        MAINNET_CHAIN_ID => "https://api.hyperliquid.xyz/info",
        TESTNET_CHAIN_ID => "https://api.hyperliquid-testnet.xyz/info",
        _ => return Err(Error::msg("unknown chain id")),
    };
    let client = reqwest::Client::new();
    let response = client
        .post(url)
        .json(&serde_json::json!({"type": "spotMeta"}))
        .send()
        .await?;
    Ok(response.json().await?)
}

pub(crate) async fn erc20_contract_to_spot_token(
    chain_id: u64,
) -> Result<BTreeMap<Address, SpotId>> {
    let meta = fetch_spot_meta(chain_id).await?;
    let mut map = BTreeMap::new();
    for token in &meta.tokens {
        if let Some(evm_contract) = &token.evm_contract {
            map.insert(evm_contract.address, SpotId { index: token.index });
        }
    }
    Ok(map)
}
