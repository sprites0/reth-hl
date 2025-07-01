use aws_config::BehaviorVersion;

use super::sources::HlNodeBlockSource;

use super::{
    consts::DEFAULT_S3_BUCKET,
    sources::{BlockSourceBoxed, LocalBlockSource, S3BlockSource},
};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct BlockSourceConfig {
    pub source_type: BlockSourceType,
    pub block_source_from_node: Option<String>,
}

#[derive(Debug, Clone)]
pub enum BlockSourceType {
    S3 { bucket: String },
    Local { path: String },
}

impl BlockSourceConfig {
    pub async fn s3_default() -> Self {
        Self {
            source_type: BlockSourceType::S3 { bucket: DEFAULT_S3_BUCKET.to_string() },
            block_source_from_node: None,
        }
    }

    pub async fn s3(bucket: String) -> Self {
        Self { source_type: BlockSourceType::S3 { bucket }, block_source_from_node: None }
    }

    pub fn local(path: String) -> Self {
        Self { source_type: BlockSourceType::Local { path }, block_source_from_node: None }
    }

    pub fn with_block_source_from_node(mut self, block_source_from_node: String) -> Self {
        self.block_source_from_node = Some(block_source_from_node);
        self
    }

    pub async fn create_block_source(&self) -> BlockSourceBoxed {
        let block_source: BlockSourceBoxed = match &self.source_type {
            BlockSourceType::S3 { bucket } => {
                let client = aws_sdk_s3::Client::new(
                    &aws_config::defaults(BehaviorVersion::latest())
                        .region("ap-northeast-1")
                        .load()
                        .await,
                );
                let block_source = S3BlockSource::new(client, bucket.clone());
                Arc::new(Box::new(block_source))
            }
            BlockSourceType::Local { path } => {
                let block_source = LocalBlockSource::new(path.clone());
                Arc::new(Box::new(block_source))
            }
        };

        let Some(block_source_from_node) = self.block_source_from_node.as_ref() else {
            return block_source;
        };

        let block_source = HlNodeBlockSource::new(
            block_source.clone(),
            PathBuf::from(block_source_from_node.clone()),
        )
        .await;
        Arc::new(Box::new(block_source))
    }

    pub async fn create_cached_block_source(&self) -> BlockSourceBoxed {
        let block_source = self.create_block_source().await;
        Arc::new(Box::new(block_source))
    }
}
