use super::{
    consts::DEFAULT_S3_BUCKET,
    sources::{
        BlockSourceBoxed, CachedBlockSource, HlNodeBlockSource, LocalBlockSource, S3BlockSource,
    },
};
use aws_config::BehaviorVersion;
use std::{env::home_dir, path::PathBuf, sync::Arc};

#[derive(Debug, Clone)]
pub struct BlockSourceConfig {
    pub source_type: BlockSourceType,
    pub block_source_from_node: Option<String>,
}

#[derive(Debug, Clone)]
pub enum BlockSourceType {
    S3 { bucket: String },
    Local { path: PathBuf },
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

    pub fn local(path: PathBuf) -> Self {
        Self { source_type: BlockSourceType::Local { path }, block_source_from_node: None }
    }

    pub fn local_default() -> Self {
        Self {
            source_type: BlockSourceType::Local {
                path: home_dir()
                    .expect("home dir not found")
                    .join("hl")
                    .join("data")
                    .join("evm_blocks_and_receipts"),
            },
            block_source_from_node: None,
        }
    }

    pub fn with_block_source_from_node(mut self, block_source_from_node: String) -> Self {
        self.block_source_from_node = Some(block_source_from_node);
        self
    }

    pub async fn create_block_source(&self) -> BlockSourceBoxed {
        match &self.source_type {
            BlockSourceType::S3 { bucket } => {
                let client = aws_sdk_s3::Client::new(
                    &aws_config::defaults(BehaviorVersion::latest())
                        .region("ap-northeast-1")
                        .load()
                        .await,
                );
                Arc::new(Box::new(S3BlockSource::new(client, bucket.clone())))
            }
            BlockSourceType::Local { path } => {
                Arc::new(Box::new(LocalBlockSource::new(path.clone())))
            }
        }
    }

    pub async fn create_block_source_from_node(
        &self,
        fallback_block_source: BlockSourceBoxed,
    ) -> BlockSourceBoxed {
        let Some(block_source_from_node) = self.block_source_from_node.as_ref() else {
            return fallback_block_source;
        };

        Arc::new(Box::new(
            HlNodeBlockSource::new(
                fallback_block_source,
                PathBuf::from(block_source_from_node.clone()),
            )
            .await,
        ))
    }

    pub async fn create_cached_block_source(&self) -> BlockSourceBoxed {
        let block_source = self.create_block_source().await;
        let block_source = self.create_block_source_from_node(block_source).await;
        Arc::new(Box::new(CachedBlockSource::new(block_source)))
    }
}
