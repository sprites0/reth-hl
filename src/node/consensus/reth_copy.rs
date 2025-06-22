//! Copy of reth codebase.

use alloy_consensus::{proofs::calculate_receipt_root, TxReceipt};
use alloy_primitives::{Bloom, B256};
use reth::consensus::ConsensusError;
use reth_primitives::GotExpected;
use reth_primitives_traits::Receipt;

/// Calculate the receipts root, and compare it against the expected receipts root and logs
/// bloom.
pub(super) fn verify_receipts<R: Receipt>(
    expected_receipts_root: B256,
    expected_logs_bloom: Bloom,
    receipts: &[R],
) -> Result<(), ConsensusError> {
    // Calculate receipts root.
    let receipts_with_bloom = receipts
        .iter()
        .map(TxReceipt::with_bloom_ref)
        .collect::<Vec<_>>();
    let receipts_root = calculate_receipt_root(&receipts_with_bloom);

    // Calculate header logs bloom.
    let logs_bloom = receipts_with_bloom
        .iter()
        .fold(Bloom::ZERO, |bloom, r| bloom | r.bloom_ref());

    compare_receipts_root_and_logs_bloom(
        receipts_root,
        logs_bloom,
        expected_receipts_root,
        expected_logs_bloom,
    )?;

    Ok(())
}

/// Compare the calculated receipts root with the expected receipts root, also compare
/// the calculated logs bloom with the expected logs bloom.
pub(super) fn compare_receipts_root_and_logs_bloom(
    calculated_receipts_root: B256,
    calculated_logs_bloom: Bloom,
    expected_receipts_root: B256,
    expected_logs_bloom: Bloom,
) -> Result<(), ConsensusError> {
    if calculated_receipts_root != expected_receipts_root {
        return Err(ConsensusError::BodyReceiptRootDiff(
            GotExpected {
                got: calculated_receipts_root,
                expected: expected_receipts_root,
            }
            .into(),
        ));
    }

    if calculated_logs_bloom != expected_logs_bloom {
        return Err(ConsensusError::BodyBloomLogDiff(
            GotExpected {
                got: calculated_logs_bloom,
                expected: expected_logs_bloom,
            }
            .into(),
        ));
    }

    Ok(())
}
