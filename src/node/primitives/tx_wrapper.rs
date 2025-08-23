//! HlNodePrimitives::TransactionSigned; it's the same as ethereum transaction type,
//! except that it supports pseudo signer for system transactions.
use alloy_consensus::{
    crypto::RecoveryError, error::ValueError, EthereumTxEnvelope, EthereumTypedTransaction,
    SignableTransaction, Signed, Transaction as TransactionTrait, TransactionEnvelope, TxEip1559,
    TxEip2930, TxEip4844, TxEip4844WithSidecar, TxEip7702, TxLegacy, TxType, TypedTransaction,
};
use alloy_eips::{eip7594::BlobTransactionSidecarVariant, Encodable2718};
use alloy_network::TxSigner;
use alloy_primitives::{address, Address, TxHash, U256};
use alloy_rpc_types::{Transaction, TransactionInfo, TransactionRequest};
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
use reth_rpc_eth_api::{
    transaction::{FromConsensusTx, TryIntoTxEnv},
    EthTxEnvError, SignTxRequestError, SignableTxRequest, TryIntoSimTx,
};
use revm::context::{BlockEnv, CfgEnv, TxEnv};

use crate::evm::transaction::HlTxEnv;

type InnerType = alloy_consensus::EthereumTxEnvelope<TxEip4844>;

#[derive(Debug, Clone, TransactionEnvelope)]
#[envelope(tx_type_name = HlTxType)]
pub enum TransactionSigned {
    #[envelope(flatten)]
    Default(InnerType),
}

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
        self.inner().recover_signer()
    }

    fn recover_signer_unchecked(&self) -> Result<Address, RecoveryError> {
        if self.is_system_transaction() {
            return Ok(s_to_address(self.signature().s()));
        }
        self.inner().recover_signer_unchecked()
    }

    fn recover_unchecked_with_buf(&self, buf: &mut Vec<u8>) -> Result<Address, RecoveryError> {
        if self.is_system_transaction() {
            return Ok(s_to_address(self.signature().s()));
        }
        self.inner().recover_unchecked_with_buf(buf)
    }
}

impl SignedTransaction for TransactionSigned {
    fn tx_hash(&self) -> &TxHash {
        self.inner().tx_hash()
    }
}

// ------------------------------------------------------------
// NOTE: All lines below are just wrappers for the inner type.
// ------------------------------------------------------------

macro_rules! impl_from_signed {
    ($($tx:ident),*) => {
        $(
            impl From<Signed<$tx>> for TransactionSigned {
                fn from(value: Signed<$tx>) -> Self {
                    Self::Default(value.into())
                }
            }
        )*
    };
}

impl_from_signed!(TxLegacy, TxEip2930, TxEip1559, TxEip7702, TypedTransaction);

impl InMemorySize for TransactionSigned {
    #[inline]
    fn size(&self) -> usize {
        self.inner().size()
    }
}

impl reth_codecs::Compact for TransactionSigned {
    fn to_compact<B>(&self, buf: &mut B) -> usize
    where
        B: bytes::BufMut + AsMut<[u8]>,
    {
        self.inner().to_compact(buf)
    }

    fn from_compact(buf: &[u8], _len: usize) -> (Self, &[u8]) {
        let (tx, hash) = InnerType::from_compact(buf, _len);
        (Self::Default(tx), hash)
    }
}

impl FromRecoveredTx<TransactionSigned> for TxEnv {
    fn from_recovered_tx(tx: &TransactionSigned, sender: Address) -> Self {
        TxEnv::from_recovered_tx(&tx.inner(), sender)
    }
}

impl FromTxCompact for TransactionSigned {
    type TxType = TxType;

    fn from_tx_compact(buf: &[u8], tx_type: Self::TxType, signature: Signature) -> (Self, &[u8])
    where
        Self: Sized,
    {
        let (tx, buf) = InnerType::from_tx_compact(buf, tx_type, signature);
        (Self::Default(tx), buf)
    }
}

impl reth_codecs::alloy::transaction::Envelope for TransactionSigned {
    fn signature(&self) -> &Signature {
        self.inner().signature()
    }

    fn tx_type(&self) -> Self::TxType {
        self.inner().tx_type()
    }
}

impl TransactionSigned {
    #[inline]
    pub fn into_inner(self) -> InnerType {
        match self {
            Self::Default(tx) => tx,
        }
    }

    #[inline]
    pub const fn inner(&self) -> &InnerType {
        match self {
            Self::Default(tx) => tx,
        }
    }

    pub fn signature(&self) -> &Signature {
        self.inner().signature()
    }

    pub const fn tx_type(&self) -> TxType {
        self.inner().tx_type()
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

impl TryFrom<TransactionSigned>
    for EthereumTxEnvelope<TxEip4844WithSidecar<BlobTransactionSidecarVariant>>
{
    type Error = <InnerType as TryInto<
        EthereumTxEnvelope<TxEip4844WithSidecar<BlobTransactionSidecarVariant>>,
    >>::Error;

    fn try_from(value: TransactionSigned) -> Result<Self, Self::Error> {
        value.into_inner().try_into()
    }
}

impl From<EthereumTxEnvelope<TxEip4844WithSidecar<BlobTransactionSidecarVariant>>>
    for TransactionSigned
{
    fn from(
        value: EthereumTxEnvelope<TxEip4844WithSidecar<BlobTransactionSidecarVariant>>,
    ) -> Self {
        Self::Default(value.into())
    }
}

impl Compress for TransactionSigned {
    type Compressed = Vec<u8>;

    fn compress(self) -> Self::Compressed {
        self.into_inner().compress()
    }

    fn compress_to_buf<B: bytes::BufMut + AsMut<[u8]>>(&self, buf: &mut B) {
        self.inner().compress_to_buf(buf);
    }
}

impl Decompress for TransactionSigned {
    fn decompress(value: &[u8]) -> Result<Self, DatabaseError> {
        Ok(Self::Default(InnerType::decompress(value)?))
    }
}

pub fn convert_to_eth_block_body(value: BlockBody) -> alloy_consensus::BlockBody<InnerType> {
    alloy_consensus::BlockBody {
        transactions: value.transactions.into_iter().map(|tx| tx.into_inner()).collect(),
        ommers: value.ommers,
        withdrawals: value.withdrawals,
    }
}

pub fn convert_to_hl_block_body(value: alloy_consensus::BlockBody<InnerType>) -> BlockBody {
    BlockBody {
        transactions: value.transactions.into_iter().map(TransactionSigned::Default).collect(),
        ommers: value.ommers,
        withdrawals: value.withdrawals,
    }
}

impl TryIntoSimTx<TransactionSigned> for TransactionRequest {
    fn try_into_sim_tx(self) -> Result<TransactionSigned, ValueError<Self>> {
        let tx = self
            .build_typed_tx()
            .map_err(|request| ValueError::new(request, "Required fields missing"))?;

        // Create an empty signature for the transaction.
        let signature = Signature::new(Default::default(), Default::default(), false);

        Ok(tx.into_signed(signature).into())
    }
}

impl TryIntoTxEnv<HlTxEnv<TxEnv>> for TransactionRequest {
    type Err = EthTxEnvError;

    fn try_into_tx_env<Spec>(
        self,
        cfg_env: &CfgEnv<Spec>,
        block_env: &BlockEnv,
    ) -> Result<HlTxEnv<TxEnv>, Self::Err> {
        Ok(HlTxEnv::new(self.clone().try_into_tx_env(cfg_env, block_env)?))
    }
}

impl FromConsensusTx<TransactionSigned> for Transaction {
    type TxInfo = TransactionInfo;

    fn from_consensus_tx(tx: TransactionSigned, signer: Address, tx_info: Self::TxInfo) -> Self {
        Self::from_transaction(Recovered::new_unchecked(tx.into_inner().into(), signer), tx_info)
    }
}

impl SignableTxRequest<TransactionSigned> for TransactionRequest {
    async fn try_build_and_sign(
        self,
        signer: impl TxSigner<Signature> + Send,
    ) -> Result<TransactionSigned, SignTxRequestError> {
        let mut tx =
            self.build_typed_tx().map_err(|_| SignTxRequestError::InvalidTransactionRequest)?;
        let signature = signer.sign_transaction(&mut tx).await?;
        let signed = match tx {
            EthereumTypedTransaction::Legacy(tx) => {
                EthereumTxEnvelope::Legacy(tx.into_signed(signature))
            }
            EthereumTypedTransaction::Eip2930(tx) => {
                EthereumTxEnvelope::Eip2930(tx.into_signed(signature))
            }
            EthereumTypedTransaction::Eip1559(tx) => {
                EthereumTxEnvelope::Eip1559(tx.into_signed(signature))
            }
            EthereumTypedTransaction::Eip4844(tx) => {
                EthereumTxEnvelope::Eip4844(TxEip4844::from(tx).into_signed(signature))
            }
            EthereumTypedTransaction::Eip7702(tx) => {
                EthereumTxEnvelope::Eip7702(tx.into_signed(signature))
            }
        };
        Ok(TransactionSigned::Default(signed))
    }
}
