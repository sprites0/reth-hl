use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::Arc,
};

use eyre::ContextCompat;
use futures::future::BoxFuture;
use reth_network::cache::LruMap;
use serde::Deserialize;
use time::{format_description, Duration, OffsetDateTime};
use tokio::sync::Mutex;
use tracing::info;

use crate::node::types::{BlockAndReceipts, EvmBlock};

use super::{BlockSource, BlockSourceBoxed};

/// Poll interval when tailing an *open* hourly file.
const TAIL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(25);
/// Sub‑directory that contains day folders (inside `local_ingest_dir`).
const HOURLY_SUBDIR: &str = "hourly";
/// Maximum number of blocks to cache blocks from hl-node.
/// In normal situation, 0~1 blocks will be cached.
const CACHE_SIZE: u32 = 1000;

/// Block source that monitors the local ingest directory for the HL node.
///
/// In certain situations, the [hl-node][ref] may offer lower latency compared to S3.
/// This block source caches blocks from the HL node to minimize latency,
/// while still falling back to [super::LocalBlockSource] or [super::S3BlockSource] when needed.
///
/// Originally introduced in https://github.com/hl-archive-node/nanoreth/pull/7
///
/// [ref]: https://github.com/hyperliquid-dex/node
#[derive(Debug, Clone)]
pub struct HlNodeBlockSource {
    pub fallback: BlockSourceBoxed,
    pub local_ingest_dir: PathBuf,
    pub local_blocks_cache: Arc<Mutex<LruMap<u64, BlockAndReceipts>>>, // height → block
}

#[derive(Deserialize)]
struct LocalBlockAndReceipts(String, BlockAndReceipts);

struct ScanResult {
    next_expected_height: u64,
    new_blocks: Vec<BlockAndReceipts>,
}

fn scan_hour_file(path: &Path, last_line: &mut usize, start_height: u64) -> ScanResult {
    // info!(
    //     "Scanning hour block file @ {:?} for height [{:?}] | Last Line {:?}",
    //     path, start_height, last_line
    // );
    let file = std::fs::File::open(path).expect("Failed to open hour file path");
    let reader = BufReader::new(file);

    let mut new_blocks = Vec::<BlockAndReceipts>::new();
    let mut last_height = start_height;
    let lines: Vec<String> = reader.lines().collect::<Result<_, _>>().unwrap();
    let skip = if *last_line == 0 { 0 } else { *last_line - 1 };

    for (line_idx, line) in lines.iter().enumerate().skip(skip) {
        // Safety check ensuring efficiency
        if line_idx < *last_line {
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }

        let LocalBlockAndReceipts(_block_timestamp, parsed_block): LocalBlockAndReceipts =
            serde_json::from_str(line).expect("Failed to parse local block and receipts");

        let height = match &parsed_block.block {
            EvmBlock::Reth115(b) => {
                let block_number = b.header.header.number;
                // Another check to ensure not returning an older block
                if block_number < start_height {
                    continue;
                }
                block_number
            }
        };
        // println!("Iterating block height {:?} | Line {}", height, line_idx);
        if height >= start_height {
            last_height = last_height.max(height);
            new_blocks.push(parsed_block);
            *last_line = line_idx;
        }
    }

    ScanResult { next_expected_height: last_height + 1, new_blocks }
}

fn datetime_from_timestamp(ts_sec: u64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp_nanos((ts_sec as i128) * 1_000 * 1_000_000)
        .expect("timestamp out of range")
}

fn date_from_datetime(dt: OffsetDateTime) -> String {
    dt.format(&format_description::parse("[year][month][day]").unwrap()).unwrap()
}

impl BlockSource for HlNodeBlockSource {
    fn collect_block(&self, height: u64) -> BoxFuture<eyre::Result<BlockAndReceipts>> {
        Box::pin(async move {
            // Not a one liner (using .or) to include logs
            if let Some(block) = self.try_collect_local_block(height).await {
                info!("Returning locally synced block for @ Height [{height}]");
                Ok(block)
            } else {
                self.fallback.collect_block(height).await
            }
        })
    }

    fn find_latest_block_number(&self) -> futures::future::BoxFuture<Option<u64>> {
        self.fallback.find_latest_block_number()
    }

    fn recommended_chunk_size(&self) -> u64 {
        self.fallback.recommended_chunk_size()
    }
}

fn to_hourly(dt: OffsetDateTime) -> Result<OffsetDateTime, time::error::ComponentRange> {
    dt.replace_minute(0)?.replace_second(0)?.replace_nanosecond(0)
}

impl HlNodeBlockSource {
    async fn try_collect_local_block(&self, height: u64) -> Option<BlockAndReceipts> {
        let mut u_cache = self.local_blocks_cache.lock().await;
        u_cache.remove(&height)
    }

    async fn start_local_ingest_loop(&self, current_head: u64, current_ts: u64) {
        let root = self.local_ingest_dir.to_owned();
        let cache = self.local_blocks_cache.clone();

        tokio::spawn(async move {
            let mut next_height = current_head;
            let mut dt = to_hourly(datetime_from_timestamp(current_ts)).unwrap();

            let mut hour = dt.hour();
            let mut day_str = date_from_datetime(dt);
            let mut last_line = 0;

            info!("Starting local ingest loop from height: {:?}", current_head);

            loop {
                let hour_file = root.join(HOURLY_SUBDIR).join(&day_str).join(format!("{hour}"));

                if hour_file.exists() {
                    let ScanResult { next_expected_height, new_blocks } =
                        scan_hour_file(&hour_file, &mut last_line, next_height);
                    if !new_blocks.is_empty() {
                        let mut u_cache = cache.lock().await;
                        for blk in new_blocks {
                            let EvmBlock::Reth115(b) = &blk.block;
                            u_cache.insert(b.header.header.number, blk);
                        }
                        next_height = next_expected_height;
                    }
                }

                // Decide whether the *current* hour file is closed (past) or
                // still live. If it’s in the past by > 1 h, move to next hour;
                // otherwise, keep tailing the same file.
                let now = OffsetDateTime::now_utc();

                // println!("Date Current {:?}", dt);
                // println!("Now Current {:?}", now);

                if dt + Duration::HOUR < now {
                    dt += Duration::HOUR;
                    hour = dt.hour();
                    day_str = date_from_datetime(dt);
                    last_line = 0;
                    info!(
                        "Moving to a new file. {:?}",
                        root.join(HOURLY_SUBDIR).join(&day_str).join(format!("{hour}"))
                    );
                    continue;
                }

                tokio::time::sleep(TAIL_INTERVAL).await;
            }
        });
    }

    pub(crate) async fn run(&self) -> eyre::Result<()> {
        let latest_block_number = self
            .fallback
            .find_latest_block_number()
            .await
            .context("Failed to find latest block number")?;

        let EvmBlock::Reth115(latest_block) =
            self.fallback.collect_block(latest_block_number).await?.block;

        let latest_block_ts = latest_block.header.header.timestamp;

        self.start_local_ingest_loop(latest_block_number, latest_block_ts).await;
        Ok(())
    }

    pub async fn new(fallback: BlockSourceBoxed, local_ingest_dir: PathBuf) -> Self {
        let block_source = HlNodeBlockSource {
            fallback,
            local_ingest_dir,
            local_blocks_cache: Arc::new(Mutex::new(LruMap::new(CACHE_SIZE))),
        };
        block_source.run().await.unwrap();
        block_source
    }
}
