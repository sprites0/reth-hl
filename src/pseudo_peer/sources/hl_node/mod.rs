mod cache;
mod file_ops;
mod scan;
#[cfg(test)]
mod tests;
mod time_utils;

use self::{
    cache::LocalBlocksCache,
    file_ops::FileOperations,
    scan::{ScanOptions, Scanner},
    time_utils::TimeUtils,
};
use super::{BlockSource, BlockSourceBoxed};
use crate::node::types::BlockAndReceipts;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use time::{Duration, OffsetDateTime};
use tokio::sync::Mutex;
use tracing::{info, warn};

const TAIL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(25);
const HOURLY_SUBDIR: &str = "hourly";
const CACHE_SIZE: u32 = 8000; // 3660 blocks per hour
const MAX_ALLOWED_THRESHOLD_BEFORE_FALLBACK: Duration = Duration::milliseconds(5000);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct LocalBlockAndReceipts(String, BlockAndReceipts);

/// Block source that monitors the local ingest directory for the HL node.
#[derive(Debug, Clone)]
pub struct HlNodeBlockSource {
    pub fallback: BlockSourceBoxed,
    pub local_ingest_dir: PathBuf,
    pub local_blocks_cache: Arc<Mutex<LocalBlocksCache>>,
    pub last_local_fetch: Arc<Mutex<Option<(u64, OffsetDateTime)>>>,
}

impl BlockSource for HlNodeBlockSource {
    fn collect_block(&self, height: u64) -> BoxFuture<'static, eyre::Result<BlockAndReceipts>> {
        let fallback = self.fallback.clone();
        let local_blocks_cache = self.local_blocks_cache.clone();
        let last_local_fetch = self.last_local_fetch.clone();
        Box::pin(async move {
            let now = OffsetDateTime::now_utc();

            if let Some(block) = Self::try_collect_local_block(local_blocks_cache, height).await {
                Self::update_last_fetch(last_local_fetch, height, now).await;
                return Ok(block);
            }

            if let Some((last_height, last_poll_time)) = *last_local_fetch.lock().await {
                let more_recent = last_height < height;
                let too_soon = now - last_poll_time < MAX_ALLOWED_THRESHOLD_BEFORE_FALLBACK;
                if more_recent && too_soon {
                    return Err(eyre::eyre!(
                            "Not found locally; limiting polling rate before fallback so that hl-node has chance to catch up"
                        ));
                }
            }

            let block = fallback.collect_block(height).await?;
            Self::update_last_fetch(last_local_fetch, height, now).await;
            Ok(block)
        })
    }

    fn find_latest_block_number(&self) -> BoxFuture<'static, Option<u64>> {
        let fallback = self.fallback.clone();
        let local_ingest_dir = self.local_ingest_dir.clone();
        Box::pin(async move {
            let Some(dir) = FileOperations::find_latest_hourly_file(&local_ingest_dir) else {
                warn!(
                    "No EVM blocks from hl-node found at {:?}; fallback to s3/ingest-dir",
                    local_ingest_dir
                );
                return fallback.find_latest_block_number().await;
            };

            match FileOperations::read_last_block_from_file(&dir) {
                Some((_, height)) => {
                    info!("Latest block number: {} with path {}", height, dir.display());
                    Some(height)
                }
                None => {
                    warn!(
                        "Failed to parse the hl-node hourly file at {:?}; fallback to s3/ingest-dir",
                        dir
                    );
                    fallback.find_latest_block_number().await
                }
            }
        })
    }

    fn recommended_chunk_size(&self) -> u64 {
        self.fallback.recommended_chunk_size()
    }
}

impl HlNodeBlockSource {
    async fn update_last_fetch(
        last_local_fetch: Arc<Mutex<Option<(u64, OffsetDateTime)>>>,
        height: u64,
        now: OffsetDateTime,
    ) {
        let mut last_fetch = last_local_fetch.lock().await;
        if last_fetch.is_none_or(|(h, _)| h < height) {
            *last_fetch = Some((height, now));
        }
    }

    async fn try_collect_local_block(
        local_blocks_cache: Arc<Mutex<LocalBlocksCache>>,
        height: u64,
    ) -> Option<BlockAndReceipts> {
        let mut u_cache = local_blocks_cache.lock().await;
        if let Some(block) = u_cache.get_block(height) {
            return Some(block);
        }
        let path = u_cache.get_path_for_height(height)?;
        info!("Loading block data from {:?}", path);
        let scan_result = Scanner::scan_hour_file(
            &path,
            &mut 0,
            ScanOptions { start_height: 0, only_load_ranges: false },
        );
        u_cache.load_scan_result(scan_result);
        u_cache.get_block(height)
    }

    async fn try_backfill_local_blocks(
        root: &Path,
        cache: &Arc<Mutex<LocalBlocksCache>>,
        cutoff_height: u64,
    ) -> eyre::Result<()> {
        let mut u_cache = cache.lock().await;
        for subfile in FileOperations::all_hourly_files(root).unwrap_or_default() {
            if let Some((_, height)) = FileOperations::read_last_block_from_file(&subfile) {
                if height < cutoff_height {
                    continue;
                }
            } else {
                warn!("Failed to parse last line of file: {:?}", subfile);
            }
            let mut scan_result = Scanner::scan_hour_file(
                &subfile,
                &mut 0,
                ScanOptions { start_height: cutoff_height, only_load_ranges: true },
            );
            scan_result.new_blocks.clear(); // Only store ranges, load data lazily
            u_cache.load_scan_result(scan_result);
        }
        u_cache.log_range_summary(root);
        Ok(())
    }

    async fn start_local_ingest_loop(&self, current_head: u64) {
        let root = self.local_ingest_dir.to_owned();
        let cache = self.local_blocks_cache.clone();
        tokio::spawn(async move {
            let mut next_height = current_head;
            let mut dt = loop {
                if let Some(f) = FileOperations::find_latest_hourly_file(&root) {
                    break TimeUtils::datetime_from_path(&f).unwrap();
                }
                tokio::time::sleep(TAIL_INTERVAL).await;
            };
            let (mut hour, mut day_str, mut last_line) =
                (dt.hour(), TimeUtils::date_from_datetime(dt), 0);
            info!("Starting local ingest loop from height: {}", current_head);
            loop {
                let hour_file = root.join(HOURLY_SUBDIR).join(&day_str).join(format!("{hour}"));
                if hour_file.exists() {
                    let scan_result = Scanner::scan_hour_file(
                        &hour_file,
                        &mut last_line,
                        ScanOptions { start_height: next_height, only_load_ranges: false },
                    );
                    next_height = scan_result.next_expected_height;
                    cache.lock().await.load_scan_result(scan_result);
                }
                let now = OffsetDateTime::now_utc();
                if dt + Duration::HOUR < now {
                    dt += Duration::HOUR;
                    (hour, day_str, last_line) = (dt.hour(), TimeUtils::date_from_datetime(dt), 0);
                    info!(
                        "Moving to new file: {:?}",
                        root.join(HOURLY_SUBDIR).join(&day_str).join(format!("{hour}"))
                    );
                    continue;
                }
                tokio::time::sleep(TAIL_INTERVAL).await;
            }
        });
    }

    pub(crate) async fn run(&self, next_block_number: u64) -> eyre::Result<()> {
        let _ = Self::try_backfill_local_blocks(
            &self.local_ingest_dir,
            &self.local_blocks_cache,
            next_block_number,
        )
        .await;
        self.start_local_ingest_loop(next_block_number).await;
        Ok(())
    }

    pub async fn new(
        fallback: BlockSourceBoxed,
        local_ingest_dir: PathBuf,
        next_block_number: u64,
    ) -> Self {
        let block_source = Self {
            fallback,
            local_ingest_dir,
            local_blocks_cache: Arc::new(Mutex::new(LocalBlocksCache::new(CACHE_SIZE))),
            last_local_fetch: Arc::new(Mutex::new(None)),
        };
        block_source.run(next_block_number).await.unwrap();
        block_source
    }
}
