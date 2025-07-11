use alloy_primitives::{BlockNumber, B256};
use reth_provider::{BlockNumReader, ProviderError};
use std::cmp::Ordering;

/// Errors that can occur in Hl consensus
#[derive(Debug, thiserror::Error)]
pub enum HlConsensusErr {
    /// Error from the provider
    #[error(transparent)]
    Provider(#[from] ProviderError),
    /// Head block hash not found
    #[error("Head block hash not found")]
    HeadHashNotFound,
}

/// Hl consensus implementation
pub struct HlConsensus<P> {
    /// The provider for reading block information
    pub provider: P,
}

impl<P> HlConsensus<P>
where
    P: BlockNumReader + Clone,
{
    /// Determines the head block hash according to Hl consensus rules:
    /// 1. Follow the highest block number
    /// 2. For same height blocks, pick the one with lower hash
    pub(crate) fn canonical_head(
        &self,
        hash: B256,
        number: BlockNumber,
    ) -> Result<(B256, B256), HlConsensusErr> {
        let current_head = self.provider.best_block_number()?;
        let current_hash =
            self.provider.block_hash(current_head)?.ok_or(HlConsensusErr::HeadHashNotFound)?;

        match number.cmp(&current_head) {
            Ordering::Greater => Ok((hash, current_hash)),
            Ordering::Equal => Ok((hash.min(current_hash), current_hash)),
            Ordering::Less => Ok((current_hash, current_hash)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::hex;
    use reth_chainspec::ChainInfo;
    use reth_provider::BlockHashReader;
    use std::collections::HashMap;

    #[derive(Clone)]
    struct MockProvider {
        blocks: HashMap<BlockNumber, B256>,
        head_number: BlockNumber,
        head_hash: B256,
    }

    impl MockProvider {
        fn new(head_number: BlockNumber, head_hash: B256) -> Self {
            let mut blocks = HashMap::new();
            blocks.insert(head_number, head_hash);
            Self { blocks, head_number, head_hash }
        }
    }

    impl BlockHashReader for MockProvider {
        fn block_hash(&self, number: BlockNumber) -> Result<Option<B256>, ProviderError> {
            Ok(self.blocks.get(&number).copied())
        }

        fn canonical_hashes_range(
            &self,
            _start: BlockNumber,
            _end: BlockNumber,
        ) -> Result<Vec<B256>, ProviderError> {
            Ok(vec![])
        }
    }

    impl BlockNumReader for MockProvider {
        fn chain_info(&self) -> Result<ChainInfo, ProviderError> {
            Ok(ChainInfo { best_hash: self.head_hash, best_number: self.head_number })
        }

        fn best_block_number(&self) -> Result<BlockNumber, ProviderError> {
            Ok(self.head_number)
        }

        fn last_block_number(&self) -> Result<BlockNumber, ProviderError> {
            Ok(self.head_number)
        }

        fn block_number(&self, hash: B256) -> Result<Option<BlockNumber>, ProviderError> {
            Ok(self.blocks.iter().find_map(|(num, h)| (*h == hash).then_some(*num)))
        }
    }

    #[test]
    fn test_canonical_head() {
        let hash1 = B256::from_slice(&hex!(
            "1111111111111111111111111111111111111111111111111111111111111111"
        ));
        let hash2 = B256::from_slice(&hex!(
            "2222222222222222222222222222222222222222222222222222222222222222"
        ));

        let test_cases = [
            ((hash1, 2, 1, hash2), hash1), // Higher block wins
            ((hash1, 1, 2, hash2), hash2), // Lower block stays
            ((hash1, 1, 1, hash2), hash1), // Same height, lower hash wins
            ((hash2, 1, 1, hash1), hash1), // Same height, lower hash stays
        ];

        for ((curr_hash, curr_num, head_num, head_hash), expected) in test_cases {
            let provider = MockProvider::new(head_num, head_hash);
            let consensus = HlConsensus { provider };
            let (head_block_hash, current_hash) =
                consensus.canonical_head(curr_hash, curr_num).unwrap();
            assert_eq!(head_block_hash, expected);
            assert_eq!(current_hash, head_hash);
        }
    }
}
