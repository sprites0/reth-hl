#![allow(clippy::owned_cow)]
use alloy_consensus::{BlobTransactionSidecar, Header};
use alloy_primitives::Address;
use alloy_rlp::{Encodable, RlpDecodable, RlpEncodable};
use reth_ethereum_primitives::Receipt;
use reth_primitives::NodePrimitives;
use reth_primitives_traits::{Block, BlockBody as BlockBodyTrait, InMemorySize};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

use crate::node::types::{ReadPrecompileCall, ReadPrecompileCalls};

pub mod tx_wrapper;
pub use tx_wrapper::{BlockBody, TransactionSigned};

/// Primitive types for HyperEVM.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct HlPrimitives;

impl NodePrimitives for HlPrimitives {
    type Block = HlBlock;
    type BlockHeader = Header;
    type BlockBody = HlBlockBody;
    type SignedTx = TransactionSigned;
    type Receipt = Receipt;
}

/// Block body for HL. It is equivalent to Ethereum [`BlockBody`] but additionally stores sidecars
/// for blob transactions.
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    derive_more::Deref,
    derive_more::DerefMut,
)]
pub struct HlBlockBody {
    #[serde(flatten)]
    #[deref]
    #[deref_mut]
    pub inner: BlockBody,
    pub sidecars: Option<Vec<BlobTransactionSidecar>>,
    pub read_precompile_calls: Option<ReadPrecompileCalls>,
    pub highest_precompile_address: Option<Address>,
}

impl InMemorySize for HlBlockBody {
    fn size(&self) -> usize {
        self.inner.size() +
            self.sidecars
                .as_ref()
                .map_or(0, |s| s.capacity() * core::mem::size_of::<BlobTransactionSidecar>()) +
            self.read_precompile_calls
                .as_ref()
                .map_or(0, |s| s.0.capacity() * core::mem::size_of::<ReadPrecompileCall>())
    }
}

impl BlockBodyTrait for HlBlockBody {
    type Transaction = TransactionSigned;
    type OmmerHeader = Header;

    fn transactions(&self) -> &[Self::Transaction] {
        BlockBodyTrait::transactions(&self.inner)
    }

    fn into_ethereum_body(self) -> BlockBody {
        self.inner
    }

    fn into_transactions(self) -> Vec<Self::Transaction> {
        self.inner.into_transactions()
    }

    fn withdrawals(&self) -> Option<&alloy_rpc_types::Withdrawals> {
        self.inner.withdrawals()
    }

    fn ommers(&self) -> Option<&[Self::OmmerHeader]> {
        self.inner.ommers()
    }

    fn calculate_tx_root(&self) -> alloy_primitives::B256 {
        alloy_consensus::proofs::calculate_transaction_root(
            &self
                .transactions()
                .iter()
                .filter(|tx| !tx.is_system_transaction())
                .collect::<Vec<_>>(),
        )
    }
}

/// Block for HL
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HlBlock {
    pub header: Header,
    pub body: HlBlockBody,
}

impl InMemorySize for HlBlock {
    fn size(&self) -> usize {
        self.header.size() + self.body.size()
    }
}

impl Block for HlBlock {
    type Header = Header;
    type Body = HlBlockBody;

    fn new(header: Self::Header, body: Self::Body) -> Self {
        Self { header, body }
    }

    fn header(&self) -> &Self::Header {
        &self.header
    }

    fn body(&self) -> &Self::Body {
        &self.body
    }

    fn split(self) -> (Self::Header, Self::Body) {
        (self.header, self.body)
    }

    fn rlp_length(header: &Self::Header, body: &Self::Body) -> usize {
        rlp::BlockHelper {
            header: Cow::Borrowed(header),
            transactions: Cow::Borrowed(&body.inner.transactions),
            ommers: Cow::Borrowed(&body.inner.ommers),
            withdrawals: body.inner.withdrawals.as_ref().map(Cow::Borrowed),
            sidecars: body.sidecars.as_ref().map(Cow::Borrowed),
            read_precompile_calls: body.read_precompile_calls.as_ref().map(Cow::Borrowed),
            highest_precompile_address: body.highest_precompile_address.as_ref().map(Cow::Borrowed),
        }
        .length()
    }
}

mod rlp {
    use super::*;
    use alloy_eips::eip4895::Withdrawals;
    use alloy_rlp::Decodable;

    #[derive(RlpEncodable, RlpDecodable)]
    #[rlp(trailing)]
    struct BlockBodyHelper<'a> {
        transactions: Cow<'a, Vec<TransactionSigned>>,
        ommers: Cow<'a, Vec<Header>>,
        withdrawals: Option<Cow<'a, Withdrawals>>,
        sidecars: Option<Cow<'a, Vec<BlobTransactionSidecar>>>,
        read_precompile_calls: Option<Cow<'a, ReadPrecompileCalls>>,
        highest_precompile_address: Option<Cow<'a, Address>>,
    }

    #[derive(RlpEncodable, RlpDecodable)]
    #[rlp(trailing)]
    pub(crate) struct BlockHelper<'a> {
        pub(crate) header: Cow<'a, Header>,
        pub(crate) transactions: Cow<'a, Vec<TransactionSigned>>,
        pub(crate) ommers: Cow<'a, Vec<Header>>,
        pub(crate) withdrawals: Option<Cow<'a, Withdrawals>>,
        pub(crate) sidecars: Option<Cow<'a, Vec<BlobTransactionSidecar>>>,
        pub(crate) read_precompile_calls: Option<Cow<'a, ReadPrecompileCalls>>,
        pub(crate) highest_precompile_address: Option<Cow<'a, Address>>,
    }

    impl<'a> From<&'a HlBlockBody> for BlockBodyHelper<'a> {
        fn from(value: &'a HlBlockBody) -> Self {
            let HlBlockBody {
                inner: BlockBody { transactions, ommers, withdrawals },
                sidecars,
                read_precompile_calls,
                highest_precompile_address,
            } = value;

            Self {
                transactions: Cow::Borrowed(transactions),
                ommers: Cow::Borrowed(ommers),
                withdrawals: withdrawals.as_ref().map(Cow::Borrowed),
                sidecars: sidecars.as_ref().map(Cow::Borrowed),
                read_precompile_calls: read_precompile_calls.as_ref().map(Cow::Borrowed),
                highest_precompile_address: highest_precompile_address.as_ref().map(Cow::Borrowed),
            }
        }
    }

    impl<'a> From<&'a HlBlock> for BlockHelper<'a> {
        fn from(value: &'a HlBlock) -> Self {
            let HlBlock {
                header,
                body:
                    HlBlockBody {
                        inner: BlockBody { transactions, ommers, withdrawals },
                        sidecars,
                        read_precompile_calls,
                        highest_precompile_address,
                    },
            } = value;

            Self {
                header: Cow::Borrowed(header),
                transactions: Cow::Borrowed(transactions),
                ommers: Cow::Borrowed(ommers),
                withdrawals: withdrawals.as_ref().map(Cow::Borrowed),
                sidecars: sidecars.as_ref().map(Cow::Borrowed),
                read_precompile_calls: read_precompile_calls.as_ref().map(Cow::Borrowed),
                highest_precompile_address: highest_precompile_address.as_ref().map(Cow::Borrowed),
            }
        }
    }

    impl Encodable for HlBlockBody {
        fn encode(&self, out: &mut dyn bytes::BufMut) {
            BlockBodyHelper::from(self).encode(out);
        }

        fn length(&self) -> usize {
            BlockBodyHelper::from(self).length()
        }
    }

    impl Decodable for HlBlockBody {
        fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
            let BlockBodyHelper {
                transactions,
                ommers,
                withdrawals,
                sidecars,
                read_precompile_calls,
                highest_precompile_address,
            } = BlockBodyHelper::decode(buf)?;
            Ok(Self {
                inner: BlockBody {
                    transactions: transactions.into_owned(),
                    ommers: ommers.into_owned(),
                    withdrawals: withdrawals.map(|w| w.into_owned()),
                },
                sidecars: sidecars.map(|s| s.into_owned()),
                read_precompile_calls: read_precompile_calls.map(|s| s.into_owned()),
                highest_precompile_address: highest_precompile_address.map(|s| s.into_owned()),
            })
        }
    }

    impl Encodable for HlBlock {
        fn encode(&self, out: &mut dyn bytes::BufMut) {
            BlockHelper::from(self).encode(out);
        }

        fn length(&self) -> usize {
            BlockHelper::from(self).length()
        }
    }

    impl Decodable for HlBlock {
        fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
            let BlockHelper {
                header,
                transactions,
                ommers,
                withdrawals,
                sidecars,
                read_precompile_calls,
                highest_precompile_address,
            } = BlockHelper::decode(buf)?;
            Ok(Self {
                header: header.into_owned(),
                body: HlBlockBody {
                    inner: BlockBody {
                        transactions: transactions.into_owned(),
                        ommers: ommers.into_owned(),
                        withdrawals: withdrawals.map(|w| w.into_owned()),
                    },
                    sidecars: sidecars.map(|s| s.into_owned()),
                    read_precompile_calls: read_precompile_calls.map(|s| s.into_owned()),
                    highest_precompile_address: highest_precompile_address.map(|s| s.into_owned()),
                },
            })
        }
    }
}

pub mod serde_bincode_compat {
    use super::*;
    use reth_primitives_traits::serde_bincode_compat::{BincodeReprFor, SerdeBincodeCompat};

    #[derive(Debug, Serialize, Deserialize)]
    pub struct HlBlockBodyBincode<'a> {
        inner: BincodeReprFor<'a, BlockBody>,
        sidecars: Option<Cow<'a, Vec<BlobTransactionSidecar>>>,
        read_precompile_calls: Option<Cow<'a, ReadPrecompileCalls>>,
        highest_precompile_address: Option<Cow<'a, Address>>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct HlBlockBincode<'a> {
        header: BincodeReprFor<'a, Header>,
        body: BincodeReprFor<'a, HlBlockBody>,
    }

    impl SerdeBincodeCompat for HlBlockBody {
        type BincodeRepr<'a> = HlBlockBodyBincode<'a>;

        fn as_repr(&self) -> Self::BincodeRepr<'_> {
            HlBlockBodyBincode {
                inner: self.inner.as_repr(),
                sidecars: self.sidecars.as_ref().map(Cow::Borrowed),
                read_precompile_calls: self.read_precompile_calls.as_ref().map(Cow::Borrowed),
                highest_precompile_address: self
                    .highest_precompile_address
                    .as_ref()
                    .map(Cow::Borrowed),
            }
        }

        fn from_repr(repr: Self::BincodeRepr<'_>) -> Self {
            let HlBlockBodyBincode {
                inner,
                sidecars,
                read_precompile_calls,
                highest_precompile_address,
            } = repr;
            Self {
                inner: BlockBody::from_repr(inner),
                sidecars: sidecars.map(|s| s.into_owned()),
                read_precompile_calls: read_precompile_calls.map(|s| s.into_owned()),
                highest_precompile_address: highest_precompile_address.map(|s| s.into_owned()),
            }
        }
    }

    impl SerdeBincodeCompat for HlBlock {
        type BincodeRepr<'a> = HlBlockBincode<'a>;

        fn as_repr(&self) -> Self::BincodeRepr<'_> {
            HlBlockBincode { header: self.header.as_repr(), body: self.body.as_repr() }
        }

        fn from_repr(repr: Self::BincodeRepr<'_>) -> Self {
            let HlBlockBincode { header, body } = repr;
            Self { header: Header::from_repr(header), body: HlBlockBody::from_repr(body) }
        }
    }
}
