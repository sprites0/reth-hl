use crate::node::types::BlockAndReceipts;
use aws_sdk_s3::types::RequestPayer;
use eyre::Context;
use futures::{future::BoxFuture, FutureExt};
use reth_network::cache::LruMap;
use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};
use tracing::info;

mod hl_node;
pub use hl_node::HlNodeBlockSource;

pub trait BlockSource: Send + Sync + std::fmt::Debug + Unpin + 'static {
    fn collect_block(&self, height: u64) -> BoxFuture<eyre::Result<BlockAndReceipts>>;
    fn find_latest_block_number(&self) -> BoxFuture<Option<u64>>;
    fn recommended_chunk_size(&self) -> u64;
}

pub type BlockSourceBoxed = Arc<Box<dyn BlockSource>>;

fn name_with_largest_number(files: &[String], is_dir: bool) -> Option<(u64, String)> {
    let mut files = files
        .iter()
        .filter_map(|file_raw| {
            let file = file_raw.strip_suffix("/").unwrap_or(file_raw).split("/").last().unwrap();
            let stem = if is_dir { file } else { file.strip_suffix(".rmp.lz4")? };
            stem.parse::<u64>().ok().map(|number| (number, file_raw.to_string()))
        })
        .collect::<Vec<_>>();
    if files.is_empty() {
        return None;
    }
    files.sort_by_key(|(number, _)| *number);
    files.last().cloned()
}

#[derive(Debug, Clone)]
pub struct S3BlockSource {
    client: aws_sdk_s3::Client,
    bucket: String,
}

impl S3BlockSource {
    pub fn new(client: aws_sdk_s3::Client, bucket: String) -> Self {
        Self { client, bucket }
    }

    async fn pick_path_with_highest_number(
        client: aws_sdk_s3::Client,
        bucket: String,
        dir: String,
        is_dir: bool,
    ) -> Option<(u64, String)> {
        let request = client
            .list_objects()
            .bucket(&bucket)
            .prefix(dir)
            .delimiter("/")
            .request_payer(RequestPayer::Requester);
        let response = request.send().await.ok()?;
        let files: Vec<String> = if is_dir {
            response
                .common_prefixes
                .unwrap()
                .iter()
                .map(|object| object.prefix.as_ref().unwrap().to_string())
                .collect()
        } else {
            response
                .contents
                .unwrap()
                .iter()
                .map(|object| object.key.as_ref().unwrap().to_string())
                .collect()
        };
        name_with_largest_number(&files, is_dir)
    }
}

impl BlockSource for S3BlockSource {
    fn collect_block(&self, height: u64) -> BoxFuture<eyre::Result<BlockAndReceipts>> {
        let client = self.client.clone();
        let bucket = self.bucket.clone();
        async move {
            let path = rmp_path(height);
            let request = client
                .get_object()
                .request_payer(RequestPayer::Requester)
                .bucket(&bucket)
                .key(path);
            let response = request.send().await?;
            let bytes = response.body.collect().await?.into_bytes();
            let mut decoder = lz4_flex::frame::FrameDecoder::new(&bytes[..]);
            let blocks: Vec<BlockAndReceipts> = rmp_serde::from_read(&mut decoder)?;
            Ok(blocks[0].clone())
        }
        .boxed()
    }

    fn find_latest_block_number(&self) -> BoxFuture<Option<u64>> {
        let client = self.client.clone();
        let bucket = self.bucket.clone();
        async move {
            let (_, first_level) = Self::pick_path_with_highest_number(
                client.clone(),
                bucket.clone(),
                "".to_string(),
                true,
            )
            .await?;
            let (_, second_level) = Self::pick_path_with_highest_number(
                client.clone(),
                bucket.clone(),
                first_level,
                true,
            )
            .await?;
            let (block_number, third_level) = Self::pick_path_with_highest_number(
                client.clone(),
                bucket.clone(),
                second_level,
                false,
            )
            .await?;

            info!("Latest block number: {} with path {}", block_number, third_level);
            Some(block_number)
        }
        .boxed()
    }

    fn recommended_chunk_size(&self) -> u64 {
        1000
    }
}

impl BlockSource for LocalBlockSource {
    fn collect_block(&self, height: u64) -> BoxFuture<eyre::Result<BlockAndReceipts>> {
        let dir = self.dir.clone();
        async move {
            let path = dir.join(rmp_path(height));
            let file = tokio::fs::read(&path)
                .await
                .wrap_err_with(|| format!("Failed to read block from {path:?}"))?;
            let mut decoder = lz4_flex::frame::FrameDecoder::new(&file[..]);
            let blocks: Vec<BlockAndReceipts> = rmp_serde::from_read(&mut decoder)?;
            Ok(blocks[0].clone())
        }
        .boxed()
    }

    fn find_latest_block_number(&self) -> BoxFuture<Option<u64>> {
        let dir = self.dir.clone();
        async move {
            let (_, first_level) = Self::pick_path_with_highest_number(dir.clone(), true).await?;
            let (_, second_level) =
                Self::pick_path_with_highest_number(dir.join(first_level), true).await?;
            let (block_number, third_level) =
                Self::pick_path_with_highest_number(dir.join(second_level), false).await?;

            info!("Latest block number: {} with path {}", block_number, third_level);
            Some(block_number)
        }
        .boxed()
    }

    fn recommended_chunk_size(&self) -> u64 {
        1000
    }
}

#[derive(Debug, Clone)]
pub struct LocalBlockSource {
    dir: PathBuf,
}

impl LocalBlockSource {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn name_with_largest_number_static(files: &[String], is_dir: bool) -> Option<(u64, String)> {
        let mut files = files
            .iter()
            .filter_map(|file_raw| {
                let file = file_raw.strip_suffix("/").unwrap_or(file_raw);
                let file = file.split("/").last().unwrap();
                let stem = if is_dir { file } else { file.strip_suffix(".rmp.lz4")? };
                stem.parse::<u64>().ok().map(|number| (number, file_raw.to_string()))
            })
            .collect::<Vec<_>>();
        if files.is_empty() {
            return None;
        }
        files.sort_by_key(|(number, _)| *number);
        files.last().map(|(number, file)| (*number, file.to_string()))
    }

    async fn pick_path_with_highest_number(dir: PathBuf, is_dir: bool) -> Option<(u64, String)> {
        let files = std::fs::read_dir(&dir).unwrap().collect::<Vec<_>>();
        let files = files
            .into_iter()
            .filter(|path| path.as_ref().unwrap().path().is_dir() == is_dir)
            .map(|entry| entry.unwrap().path().to_string_lossy().to_string())
            .collect::<Vec<_>>();

        Self::name_with_largest_number_static(&files, is_dir)
    }
}

fn rmp_path(height: u64) -> String {
    let f = ((height - 1) / 1_000_000) * 1_000_000;
    let s = ((height - 1) / 1_000) * 1_000;
    let path = format!("{f}/{s}/{height}.rmp.lz4");
    path
}

impl BlockSource for BlockSourceBoxed {
    fn collect_block(&self, height: u64) -> BoxFuture<eyre::Result<BlockAndReceipts>> {
        self.as_ref().collect_block(height)
    }

    fn find_latest_block_number(&self) -> BoxFuture<Option<u64>> {
        self.as_ref().find_latest_block_number()
    }

    fn recommended_chunk_size(&self) -> u64 {
        self.as_ref().recommended_chunk_size()
    }
}

#[derive(Debug, Clone)]
pub struct CachedBlockSource {
    block_source: BlockSourceBoxed,
    cache: Arc<RwLock<LruMap<u64, BlockAndReceipts>>>,
}

impl CachedBlockSource {
    const CACHE_LIMIT: u32 = 100000;
    pub fn new(block_source: BlockSourceBoxed) -> Self {
        Self { block_source, cache: Arc::new(RwLock::new(LruMap::new(Self::CACHE_LIMIT))) }
    }
}

impl BlockSource for CachedBlockSource {
    fn collect_block(&self, height: u64) -> BoxFuture<eyre::Result<BlockAndReceipts>> {
        let block_source = self.block_source.clone();
        let cache = self.cache.clone();
        async move {
            if let Some(block) = cache.write().unwrap().get(&height) {
                return Ok(block.clone());
            }
            let block = block_source.collect_block(height).await?;
            cache.write().unwrap().insert(height, block.clone());
            Ok(block)
        }
        .boxed()
    }

    fn find_latest_block_number(&self) -> BoxFuture<Option<u64>> {
        self.block_source.find_latest_block_number()
    }

    fn recommended_chunk_size(&self) -> u64 {
        self.block_source.recommended_chunk_size()
    }
}
