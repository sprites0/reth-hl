//! Extends from https://github.com/hyperliquid-dex/hyper-evm-sync
//!
//! Changes:
//! - ReadPrecompileCalls supports RLP encoding / decoding
use alloy_primitives::{Address, Bytes, Log};
use alloy_rlp::{Decodable, Encodable, RlpDecodable, RlpEncodable};
use bytes::BufMut;
use reth_primitives::{SealedBlock, Transaction};
use serde::{Deserialize, Serialize};

pub type ReadPrecompileCall = (Address, Vec<(ReadPrecompileInput, ReadPrecompileResult)>);

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default)]
pub struct ReadPrecompileCalls(pub Vec<ReadPrecompileCall>);

impl Encodable for ReadPrecompileCalls {
    fn encode(&self, out: &mut dyn BufMut) {
        rmp_serde::encode::write(&mut out.writer(), &self.0).unwrap();
    }
}

impl Decodable for ReadPrecompileCalls {
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        let calls = rmp_serde::decode::from_slice(buf)
            .map_err(|_| alloy_rlp::Error::Custom("Failed to decode ReadPrecompileCalls"))?;
        Ok(Self(calls))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockAndReceipts {
    pub block: EvmBlock,
    pub receipts: Vec<LegacyReceipt>,
    #[serde(default)]
    pub system_txs: Vec<SystemTx>,
    #[serde(default)]
    pub read_precompile_calls: ReadPrecompileCalls,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EvmBlock {
    Reth115(SealedBlock),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyReceipt {
    tx_type: LegacyTxType,
    success: bool,
    cumulative_gas_used: u64,
    logs: Vec<Log>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum LegacyTxType {
    Legacy = 0,
    Eip2930 = 1,
    Eip1559 = 2,
    Eip4844 = 3,
    Eip7702 = 4,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemTx {
    pub tx: Transaction,
    pub receipt: Option<LegacyReceipt>,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    Hash,
    RlpEncodable,
    RlpDecodable,
)]
pub struct ReadPrecompileInput {
    pub input: Bytes,
    pub gas_limit: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum ReadPrecompileResult {
    Ok { gas_used: u64, bytes: Bytes },
    OutOfGas,
    Error,
    UnexpectedError,
}
