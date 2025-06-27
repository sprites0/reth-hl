//! HlNodePrimitives::TransactionSigned; it's the same as ethereum transaction type,
//! except that it supports pseudo signer for system transactions.
use std::hash::Hasher;

use alloy_consensus::{
    crypto::RecoveryError, EthereumTxEnvelope, Signed, Transaction as TransactionTrait, TxEip1559,
    TxEip2930, TxEip4844, TxEip4844WithSidecar, TxEip7702, TxEnvelope, TxLegacy, TxType,
    TypedTransaction,
};
use alloy_eips::{
    eip2718::Eip2718Result, eip7594::BlobTransactionSidecarVariant, eip7702::SignedAuthorization,
    Decodable2718, Encodable2718, Typed2718,
};
use alloy_primitives::{address, Address, Bytes, TxHash, TxKind, Uint, B256, U256};
use alloy_rpc_types::AccessList;
use alloy_signer::Signature;
use reth_codecs::alloy::transaction::FromTxCompact;
use reth_db::{
    table::{Compress, Decompress},
    DatabaseError,
};
use reth_evm::FromRecoveredTx;
use reth_primitives::Recovered;
use reth_primitives_traits::{
    serde_bincode_compat::SerdeBincodeCompat, InMemorySize, SignedTransaction, SignerRecoverable,
};
use revm::context::TxEnv;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

type InnerType = alloy_consensus::EthereumTxEnvelope<TxEip4844>;

#[derive(Debug, Clone, Eq)]
pub struct TransactionSigned(pub InnerType);

fn s_to_address(s: U256) -> Address {
    if s == U256::ONE {
        return address!("2222222222222222222222222222222222222222");
    }
    let mut buf = [0u8; 20];
    buf[0..20].copy_from_slice(&s.to_be_bytes::<32>()[12..32]);
    Address::from_slice(&buf)
}

impl SignerRecoverable for TransactionSigned {
    fn recover_signer(&self) -> Result<Address, RecoveryError> {
        if self.is_system_transaction() {
            return Ok(s_to_address(self.signature().s()));
        }
        self.0.recover_signer()
    }

    fn recover_signer_unchecked(&self) -> Result<Address, RecoveryError> {
        if self.is_system_transaction() {
            return Ok(s_to_address(self.signature().s()));
        }
        self.0.recover_signer_unchecked()
    }
}

impl SignedTransaction for TransactionSigned {
    fn tx_hash(&self) -> &TxHash {
        self.0.tx_hash()
    }

    fn recover_signer_unchecked_with_buf(
        &self,
        buf: &mut Vec<u8>,
    ) -> Result<Address, RecoveryError> {
        if self.is_system_transaction() {
            return Ok(s_to_address(self.signature().s()));
        }

        self.0.recover_signer_unchecked_with_buf(buf)
    }
}

// ------------------------------------------------------------
// NOTE: All lines below are just wrappers for the inner type.
// ------------------------------------------------------------

impl Serialize for TransactionSigned {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TransactionSigned {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self(InnerType::deserialize(deserializer)?))
    }
}

macro_rules! impl_from_signed {
    ($($tx:ident),*) => {
        $(
            impl From<Signed<$tx>> for TransactionSigned {
                fn from(value: Signed<$tx>) -> Self {
                    Self(value.into())
                }
            }
        )*
    };
}

impl_from_signed!(TxLegacy, TxEip2930, TxEip1559, TxEip7702, TypedTransaction);

impl InMemorySize for TransactionSigned {
    #[inline]
    fn size(&self) -> usize {
        self.0.size()
    }
}

impl alloy_rlp::Encodable for TransactionSigned {
    fn encode(&self, out: &mut dyn alloy_rlp::bytes::BufMut) {
        self.0.encode(out);
    }

    fn length(&self) -> usize {
        self.0.length()
    }
}

impl alloy_rlp::Decodable for TransactionSigned {
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        Ok(Self(TxEnvelope::decode(buf)?.into()))
    }
}

impl Encodable2718 for TransactionSigned {
    fn type_flag(&self) -> Option<u8> {
        self.0.type_flag()
    }

    fn encode_2718_len(&self) -> usize {
        self.0.encode_2718_len()
    }

    fn encode_2718(&self, out: &mut dyn alloy_rlp::BufMut) {
        self.0.encode_2718(out)
    }
}

impl Decodable2718 for TransactionSigned {
    fn typed_decode(ty: u8, buf: &mut &[u8]) -> Eip2718Result<Self> {
        Ok(Self(TxEnvelope::typed_decode(ty, buf)?.into()))
    }

    fn fallback_decode(buf: &mut &[u8]) -> Eip2718Result<Self> {
        Ok(Self(TxEnvelope::fallback_decode(buf)?.into()))
    }
}

impl Typed2718 for TransactionSigned {
    fn ty(&self) -> u8 {
        self.0.ty()
    }
}

impl PartialEq for TransactionSigned {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl core::hash::Hash for TransactionSigned {
    fn hash<H: Hasher>(&self, state: &mut H) {
        core::hash::Hash::hash(&self.0, state);
    }
}

impl TransactionTrait for TransactionSigned {
    fn chain_id(&self) -> Option<u64> {
        self.0.chain_id()
    }

    fn nonce(&self) -> u64 {
        self.0.nonce()
    }

    fn gas_limit(&self) -> u64 {
        self.0.gas_limit()
    }

    fn gas_price(&self) -> Option<u128> {
        self.0.gas_price()
    }

    fn max_fee_per_gas(&self) -> u128 {
        self.0.max_fee_per_gas()
    }

    fn max_priority_fee_per_gas(&self) -> Option<u128> {
        self.0.max_priority_fee_per_gas()
    }

    fn max_fee_per_blob_gas(&self) -> Option<u128> {
        self.0.max_fee_per_blob_gas()
    }

    fn priority_fee_or_price(&self) -> u128 {
        self.0.priority_fee_or_price()
    }

    fn effective_gas_price(&self, base_fee: Option<u64>) -> u128 {
        self.0.effective_gas_price(base_fee)
    }

    fn effective_tip_per_gas(&self, base_fee: u64) -> Option<u128> {
        self.0.effective_tip_per_gas(base_fee)
    }

    fn is_dynamic_fee(&self) -> bool {
        self.0.is_dynamic_fee()
    }

    fn kind(&self) -> TxKind {
        self.0.kind()
    }

    fn is_create(&self) -> bool {
        self.0.is_create()
    }

    fn value(&self) -> Uint<256, 4> {
        self.0.value()
    }

    fn input(&self) -> &Bytes {
        self.0.input()
    }

    fn access_list(&self) -> Option<&AccessList> {
        self.0.access_list()
    }

    fn blob_versioned_hashes(&self) -> Option<&[B256]> {
        self.0.blob_versioned_hashes()
    }

    fn authorization_list(&self) -> Option<&[SignedAuthorization]> {
        self.0.authorization_list()
    }
}

impl reth_codecs::Compact for TransactionSigned {
    fn to_compact<B>(&self, buf: &mut B) -> usize
    where
        B: bytes::BufMut + AsMut<[u8]>,
    {
        self.0.to_compact(buf)
    }

    fn from_compact(buf: &[u8], _len: usize) -> (Self, &[u8]) {
        let (tx, hash) = InnerType::from_compact(buf, _len);
        (Self(tx), hash)
    }
}

pub fn convert_recovered(value: Recovered<TransactionSigned>) -> Recovered<InnerType> {
    let (tx, signer) = value.into_parts();
    Recovered::new_unchecked(tx.0, signer)
}

impl FromRecoveredTx<TransactionSigned> for TxEnv {
    fn from_recovered_tx(tx: &TransactionSigned, sender: Address) -> Self {
        TxEnv::from_recovered_tx(&tx.0, sender)
    }
}

impl FromTxCompact for TransactionSigned {
    type TxType = TxType;

    fn from_tx_compact(buf: &[u8], tx_type: Self::TxType, signature: Signature) -> (Self, &[u8])
    where
        Self: Sized,
    {
        let (tx, buf) = InnerType::from_tx_compact(buf, tx_type, signature);
        (Self(tx), buf)
    }
}

impl reth_codecs::alloy::transaction::Envelope for TransactionSigned {
    fn signature(&self) -> &Signature {
        self.0.signature()
    }

    fn tx_type(&self) -> Self::TxType {
        self.0.tx_type()
    }
}

impl TransactionSigned {
    pub const fn signature(&self) -> &Signature {
        self.0.signature()
    }

    pub const fn tx_type(&self) -> TxType {
        self.0.tx_type()
    }

    pub fn is_system_transaction(&self) -> bool {
        self.gas_price().is_some() && self.gas_price().unwrap() == 0
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct BincodeCompatTxCustom(pub TransactionSigned);

impl SerdeBincodeCompat for TransactionSigned {
    type BincodeRepr<'a> = BincodeCompatTxCustom;

    fn as_repr(&self) -> Self::BincodeRepr<'_> {
        BincodeCompatTxCustom(self.clone())
    }

    fn from_repr(repr: Self::BincodeRepr<'_>) -> Self {
        repr.0
    }
}

pub type BlockBody = alloy_consensus::BlockBody<TransactionSigned>;

impl From<TransactionSigned> for EthereumTxEnvelope<TxEip4844> {
    fn from(value: TransactionSigned) -> Self {
        value.0
    }
}

impl TryFrom<TransactionSigned> for EthereumTxEnvelope<TxEip4844WithSidecar> {
    type Error = <InnerType as TryInto<EthereumTxEnvelope<TxEip4844WithSidecar>>>::Error;

    fn try_from(value: TransactionSigned) -> Result<Self, Self::Error> {
        value.0.try_into()
    }
}

impl TryFrom<TransactionSigned>
    for EthereumTxEnvelope<TxEip4844WithSidecar<BlobTransactionSidecarVariant>>
{
    type Error = <InnerType as TryInto<
        EthereumTxEnvelope<TxEip4844WithSidecar<BlobTransactionSidecarVariant>>,
    >>::Error;

    fn try_from(value: TransactionSigned) -> Result<Self, Self::Error> {
        value.0.try_into()
    }
}

impl From<EthereumTxEnvelope<TxEip4844WithSidecar<BlobTransactionSidecarVariant>>>
    for TransactionSigned
{
    fn from(
        value: EthereumTxEnvelope<TxEip4844WithSidecar<BlobTransactionSidecarVariant>>,
    ) -> Self {
        Self(value.into())
    }
}

impl Compress for TransactionSigned {
    type Compressed = Vec<u8>;

    fn compress(self) -> Self::Compressed {
        self.0.compress()
    }

    fn compress_to_buf<B: bytes::BufMut + AsMut<[u8]>>(&self, buf: &mut B) {
        self.0.compress_to_buf(buf);
    }
}

impl Decompress for TransactionSigned {
    fn decompress(value: &[u8]) -> Result<Self, DatabaseError> {
        Ok(Self(InnerType::decompress(value)?))
    }
}

pub fn convert_to_eth_block_body(value: BlockBody) -> alloy_consensus::BlockBody<InnerType> {
    alloy_consensus::BlockBody {
        transactions: value.transactions.into_iter().map(|tx| tx.0).collect(),
        ommers: value.ommers,
        withdrawals: value.withdrawals,
    }
}

pub fn convert_to_hl_block_body(value: alloy_consensus::BlockBody<InnerType>) -> BlockBody {
    BlockBody {
        transactions: value.transactions.into_iter().map(TransactionSigned).collect(),
        ommers: value.ommers,
        withdrawals: value.withdrawals,
    }
}
