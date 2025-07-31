use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
};

use futures::future::BoxFuture;
use serde::Deserialize;
use time::{macros::format_description, Date, Duration, OffsetDateTime, Time};
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::node::types::{BlockAndReceipts, EvmBlock};

use super::{BlockSource, BlockSourceBoxed};

const TAIL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(25);
const HOURLY_SUBDIR: &str = "hourly";

type LocalBlocksCache = Arc<Mutex<HashMap<u64, BlockAndReceipts>>>;

/// Block source that monitors the local ingest directory for the HL node.
#[derive(Debug, Clone)]
pub struct HlNodeBlockSource {
    pub fallback: BlockSourceBoxed,
    pub local_ingest_dir: PathBuf,
    pub local_blocks_cache: LocalBlocksCache, // height → block
}

#[derive(Deserialize)]
struct LocalBlockAndReceipts(String, BlockAndReceipts);

struct ScanResult {
    next_expected_height: u64,
    new_blocks: Vec<BlockAndReceipts>,
}

fn line_to_evm_block(line: &str) -> serde_json::Result<(BlockAndReceipts, u64)> {
    let LocalBlockAndReceipts(_block_timestamp, parsed_block): LocalBlockAndReceipts =
        serde_json::from_str(line)?;
    let height = match &parsed_block.block {
        EvmBlock::Reth115(b) => b.header.header.number,
    };
    Ok((parsed_block, height))
}

fn scan_hour_file(path: &Path, last_line: &mut usize, start_height: u64) -> ScanResult {
    let file = File::open(path).expect("Failed to open hour file path");
    let reader = BufReader::new(file);

    let mut new_blocks = Vec::new();
    let mut last_height = start_height;
    let lines: Vec<String> = reader.lines().collect::<Result<_, _>>().unwrap();
    let skip = if *last_line == 0 { 0 } else { *last_line - 1 };

    for (line_idx, line) in lines.iter().enumerate().skip(skip) {
        if line_idx < *last_line || line.trim().is_empty() {
            continue;
        }

        match line_to_evm_block(line) {
            Ok((parsed_block, height)) if height >= start_height => {
                last_height = last_height.max(height);
                new_blocks.push(parsed_block);
                *last_line = line_idx;
            }
            Ok(_) => continue,
            Err(_) => {
                warn!("Failed to parse line: {}...", line.get(0..50).unwrap_or(line));
                continue;
            }
        }
    }

    ScanResult { next_expected_height: last_height + 1, new_blocks }
}

fn date_from_datetime(dt: OffsetDateTime) -> String {
    dt.format(&format_description!("[year][month][day]")).unwrap()
}

impl BlockSource for HlNodeBlockSource {
    fn collect_block(&self, height: u64) -> BoxFuture<eyre::Result<BlockAndReceipts>> {
        Box::pin(async move {
            if let Some(block) = self.try_collect_local_block(height).await {
                info!("Returning locally synced block for @ Height [{height}]");
                Ok(block)
            } else {
                self.fallback.collect_block(height).await
            }
        })
    }

    fn find_latest_block_number(&self) -> BoxFuture<Option<u64>> {
        Box::pin(async move {
            let Some(dir) = Self::find_latest_hourly_file(&self.local_ingest_dir) else {
                warn!(
                    "No EVM blocks from hl-node found at {:?}; fallback to s3/ingest-dir",
                    self.local_ingest_dir
                );
                return self.fallback.find_latest_block_number().await;
            };
            let mut file = File::open(&dir).expect("Failed to open hour file path");
            let last_line = read_last_complete_line(&mut file);
            let Ok((_, height)) = line_to_evm_block(&last_line) else {
                warn!(
                    "Failed to parse the hl-node hourly file at {:?}; fallback to s3/ingest-dir",
                    file
                );
                return self.fallback.find_latest_block_number().await;
            };

            info!("Latest block number: {} with path {}", height, dir.display());
            Some(height)
        })
    }

    fn recommended_chunk_size(&self) -> u64 {
        self.fallback.recommended_chunk_size()
    }
}

fn read_last_complete_line<R: Read + Seek>(read: &mut R) -> String {
    const CHUNK_SIZE: u64 = 4096;
    let mut buf = Vec::with_capacity(CHUNK_SIZE as usize);
    let mut pos = read.seek(SeekFrom::End(0)).unwrap();
    let mut last_line: Vec<u8> = Vec::new();

    while pos > 0 {
        let read_size = std::cmp::min(pos, CHUNK_SIZE);
        buf.resize(read_size as usize, 0);

        read.seek(SeekFrom::Start(pos - read_size)).unwrap();
        read.read_exact(&mut buf).unwrap();

        last_line = [buf.clone(), last_line].concat();

        if last_line.ends_with(b"\n") {
            last_line.pop();
        }

        if let Some(idx) = last_line.iter().rposition(|&b| b == b'\n') {
            let candidate = &last_line[idx + 1..];
            if line_to_evm_block(&String::from_utf8(candidate.to_vec()).unwrap()).is_ok() {
                return String::from_utf8(candidate.to_vec()).unwrap();
            }
            last_line.truncate(idx);
        }

        if pos < read_size {
            break;
        }
        pos -= read_size;
    }

    String::from_utf8(last_line).unwrap()
}

impl HlNodeBlockSource {
    async fn try_collect_local_block(&self, height: u64) -> Option<BlockAndReceipts> {
        let mut u_cache = self.local_blocks_cache.lock().await;
        u_cache.remove(&height)
    }

    fn datetime_from_path(path: &Path) -> Option<OffsetDateTime> {
        let dt_part = path.parent()?.file_name()?.to_str()?;
        let hour_part = path.file_name()?.to_str()?;

        let hour: u8 = hour_part.parse().ok()?;
        Some(OffsetDateTime::new_utc(
            Date::parse(&format!("{dt_part}"), &format_description!("[year][month][day]")).ok()?,
            Time::from_hms(hour, 0, 0).ok()?,
        ))
    }

    fn all_hourly_files(root: &Path) -> Option<Vec<PathBuf>> {
        let dir = root.join(HOURLY_SUBDIR);
        let mut files = Vec::new();
        for entry in std::fs::read_dir(dir).ok()? {
            let file = entry.ok()?.path();
            let subfiles: Vec<_> = std::fs::read_dir(&file)
                .ok()?
                .filter_map(|f| f.ok().map(|f| f.path()))
                .filter(|p| Self::datetime_from_path(p).is_some())
                .collect();
            files.extend(subfiles);
        }
        files.sort();
        Some(files)
    }

    fn find_latest_hourly_file(root: &Path) -> Option<PathBuf> {
        Self::all_hourly_files(root)?.last().cloned()
    }

    async fn try_backfill_local_blocks(
        root: &Path,
        cache: &LocalBlocksCache,
        mut next_height: u64,
    ) -> eyre::Result<()> {
        let mut u_cache = cache.lock().await;

        for subfile in Self::all_hourly_files(root).unwrap_or_default() {
            let mut file = File::open(&subfile).expect("Failed to open hour file path");
            let last_line = read_last_complete_line(&mut file);
            if let Ok((_, height)) = line_to_evm_block(&last_line) {
                if height < next_height {
                    continue;
                }
            } else {
                warn!("Failed to parse last line of file, fallback to slow path: {:?}", subfile);
            }

            let ScanResult { next_expected_height, new_blocks } =
                scan_hour_file(&subfile, &mut 0, next_height);
            for blk in new_blocks {
                let EvmBlock::Reth115(b) = &blk.block;
                u_cache.insert(b.header.header.number, blk);
            }
            next_height = next_expected_height;
        }
        info!("Backfilled {} blocks", u_cache.len());

        Ok(())
    }

    async fn start_local_ingest_loop(&self, current_head: u64) {
        let root = self.local_ingest_dir.to_owned();
        let cache = self.local_blocks_cache.clone();

        tokio::spawn(async move {
            let mut next_height = current_head;

            // Wait for the first hourly file to be created
            let mut dt = loop {
                if let Some(latest_file) = Self::find_latest_hourly_file(&root) {
                    break Self::datetime_from_path(&latest_file).unwrap();
                }
                tokio::time::sleep(TAIL_INTERVAL).await;
            };

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

                let now = OffsetDateTime::now_utc();

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
        let block_source = HlNodeBlockSource {
            fallback,
            local_ingest_dir,
            local_blocks_cache: Arc::new(Mutex::new(HashMap::new())),
        };
        block_source.run(next_block_number).await.unwrap();
        block_source
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_datetime_from_path() {
        let path = Path::new("/home/username/hl/data/evm_block_and_receipts/hourly/20250731/4");
        let dt = HlNodeBlockSource::datetime_from_path(path).unwrap();
        println!("{:?}", dt);
    }
}
