//! Copy of reth codebase to preserve serialization compatibility
use alloy_consensus::{Header, Signed, TxEip1559, TxEip2930, TxEip4844, TxEip7702, TxLegacy};
use alloy_primitives::{Address, BlockHash, Signature, TxKind, U256};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock, Mutex},
};
use tracing::info;

use crate::{
    node::{
        spot_meta::{erc20_contract_to_spot_token, SpotId},
        types::{ReadPrecompileCalls, SystemTx},
    },
    HlBlock, HlBlockBody,
};

/// A raw transaction.
///
/// Transaction types were introduced in [EIP-2718](https://eips.ethereum.org/EIPS/eip-2718).
#[derive(Debug, Clone, PartialEq, Eq, Hash, derive_more::From, Serialize, Deserialize)]
pub enum Transaction {
    Legacy(TxLegacy),
    Eip2930(TxEip2930),
    Eip1559(TxEip1559),
    Eip4844(TxEip4844),
    Eip7702(TxEip7702),
}

/// Signed Ethereum transaction.
#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, derive_more::AsRef, derive_more::Deref,
)]
#[serde(rename_all = "camelCase")]
pub struct TransactionSigned {
    /// The transaction signature values
    signature: Signature,
    /// Raw transaction info
    #[deref]
    #[as_ref]
    transaction: Transaction,
}
impl TransactionSigned {
    fn to_reth_transaction(&self) -> reth_primitives::TransactionSigned {
        match self.transaction.clone() {
            Transaction::Legacy(tx) => {
                reth_primitives::TransactionSigned::Legacy(Signed::new_unhashed(tx, self.signature))
            }
            Transaction::Eip2930(tx) => reth_primitives::TransactionSigned::Eip2930(
                Signed::new_unhashed(tx, self.signature),
            ),
            Transaction::Eip1559(tx) => reth_primitives::TransactionSigned::Eip1559(
                Signed::new_unhashed(tx, self.signature),
            ),
            Transaction::Eip4844(tx) => reth_primitives::TransactionSigned::Eip4844(
                Signed::new_unhashed(tx, self.signature),
            ),
            Transaction::Eip7702(tx) => reth_primitives::TransactionSigned::Eip7702(
                Signed::new_unhashed(tx, self.signature),
            ),
        }
    }
}

type BlockBody = alloy_consensus::BlockBody<TransactionSigned, Header>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SealedHeader {
    hash: BlockHash,
    header: Header,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SealedBlock {
    /// Sealed Header.
    header: SealedHeader,
    /// the block's body.
    body: BlockBody,
}

fn system_tx_to_reth_transaction(
    transaction: &SystemTx,
    chain_id: u64,
) -> reth_primitives::TransactionSigned {
    static EVM_MAP: LazyLock<Arc<Mutex<BTreeMap<Address, SpotId>>>> =
        LazyLock::new(|| Arc::new(Mutex::new(BTreeMap::new())));
    {
        let Transaction::Legacy(tx) = &transaction.tx else {
            panic!("Unexpected transaction type");
        };
        let TxKind::Call(to) = tx.to else {
            panic!("Unexpected contract creation");
        };
        let s = if tx.input.is_empty() {
            U256::from(0x1)
        } else {
            loop {
                if let Some(spot) = EVM_MAP.lock().unwrap().get(&to) {
                    break spot.to_s();
                }

                info!("Contract not found: {:?} from spot mapping, fetching again...", to);
                *EVM_MAP.lock().unwrap() = erc20_contract_to_spot_token(chain_id).unwrap();
            }
        };
        let signature = Signature::new(U256::from(0x1), s, true);
        reth_primitives::TransactionSigned::Legacy(Signed::new_unhashed(tx.clone(), signature))
    }
}

impl SealedBlock {
    pub fn to_reth_block(
        &self,
        read_precompile_calls: ReadPrecompileCalls,
        system_txs: Vec<super::SystemTx>,
        chain_id: u64,
    ) -> HlBlock {
        let mut merged_txs = vec![];
        merged_txs.extend(system_txs.iter().map(|tx| system_tx_to_reth_transaction(tx, chain_id)));
        merged_txs.extend(self.body.transactions.iter().map(|tx| tx.to_reth_transaction()));
        let block_body = HlBlockBody {
            inner: reth_primitives::BlockBody {
                transactions: merged_txs,
                withdrawals: self.body.withdrawals.clone(),
                ommers: self.body.ommers.clone(),
            },
            sidecars: None,
            read_precompile_calls: Some(read_precompile_calls),
        };

        HlBlock { header: self.header.header.clone(), body: block_body }
    }
}
