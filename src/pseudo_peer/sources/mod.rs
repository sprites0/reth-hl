use crate::node::types::BlockAndReceipts;
use futures::future::BoxFuture;
use std::sync::Arc;

// Module declarations
mod cached;
mod hl_node;
mod local;
mod s3;
mod utils;

// Public exports
pub use cached::CachedBlockSource;
pub use hl_node::HlNodeBlockSource;
pub use local::LocalBlockSource;
pub use s3::S3BlockSource;

/// Trait for block sources that can retrieve blocks from various sources
pub trait BlockSource: Send + Sync + std::fmt::Debug + Unpin + 'static {
    /// Retrieves a block at the specified height
    fn collect_block(&self, height: u64) -> BoxFuture<'static, eyre::Result<BlockAndReceipts>>;

    /// Finds the latest block number available from this source
    fn find_latest_block_number(&self) -> BoxFuture<'static, Option<u64>>;

    /// Returns the recommended chunk size for batch operations
    fn recommended_chunk_size(&self) -> u64;
}

/// Type alias for a boxed block source
pub type BlockSourceBoxed = Arc<Box<dyn BlockSource>>;

impl BlockSource for BlockSourceBoxed {
    fn collect_block(&self, height: u64) -> BoxFuture<'static, eyre::Result<BlockAndReceipts>> {
        self.as_ref().collect_block(height)
    }

    fn find_latest_block_number(&self) -> BoxFuture<'static, Option<u64>> {
        self.as_ref().find_latest_block_number()
    }

    fn recommended_chunk_size(&self) -> u64 {
        self.as_ref().recommended_chunk_size()
    }
}
