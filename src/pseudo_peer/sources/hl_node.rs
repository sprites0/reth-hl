use super::{BlockSource, BlockSourceBoxed};
use crate::node::types::{BlockAndReceipts, EvmBlock};
use futures::future::BoxFuture;
use rangemap::RangeInclusiveMap;
use reth_network::cache::LruMap;
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    ops::RangeInclusive,
    path::{Path, PathBuf},
    sync::Arc,
};
use time::{macros::format_description, Date, Duration, OffsetDateTime, Time};
use tokio::sync::Mutex;
use tracing::{info, warn};

const TAIL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(25);
const HOURLY_SUBDIR: &str = "hourly";

#[derive(Debug)]
pub struct LocalBlocksCache {
    cache: LruMap<u64, BlockAndReceipts>,
    // Lightweight range map to track the ranges of blocks in the local ingest directory
    ranges: RangeInclusiveMap<u64, PathBuf>,
}

impl LocalBlocksCache {
    // 3660 blocks per hour
    const CACHE_SIZE: u32 = 8000;

    fn new() -> Self {
        Self { cache: LruMap::new(Self::CACHE_SIZE), ranges: RangeInclusiveMap::new() }
    }

    fn load_scan_result(&mut self, scan_result: ScanResult) {
        for blk in scan_result.new_blocks {
            let EvmBlock::Reth115(b) = &blk.block;
            self.cache.insert(b.header.header.number, blk);
        }
        for range in scan_result.new_block_ranges {
            self.ranges.insert(range, scan_result.path.clone());
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct LocalBlockAndReceipts(String, BlockAndReceipts);

struct ScanResult {
    path: PathBuf,
    next_expected_height: u64,
    new_blocks: Vec<BlockAndReceipts>,
    new_block_ranges: Vec<RangeInclusive<u64>>,
}

struct ScanOptions {
    start_height: u64,
    only_load_ranges: bool,
}

fn line_to_evm_block(line: &str) -> serde_json::Result<(BlockAndReceipts, u64)> {
    let LocalBlockAndReceipts(_block_timestamp, parsed_block): LocalBlockAndReceipts =
        serde_json::from_str(line)?;
    let height = match &parsed_block.block {
        EvmBlock::Reth115(b) => b.header.header.number,
    };
    Ok((parsed_block, height))
}

fn scan_hour_file(path: &Path, last_line: &mut usize, options: ScanOptions) -> ScanResult {
    let file = File::open(path).expect("Failed to open hour file path");
    let reader = BufReader::new(file);

    let ScanOptions { start_height, only_load_ranges } = options;

    let mut new_blocks = Vec::new();
    let mut last_height = start_height;
    let lines: Vec<String> = reader.lines().collect::<Result<_, _>>().unwrap();
    let skip = if *last_line == 0 { 0 } else { *last_line - 1 };

    let mut block_ranges = Vec::new();
    let mut current_range: Option<(u64, u64)> = None;

    for (line_idx, line) in lines.iter().enumerate().skip(skip) {
        if line_idx < *last_line || line.trim().is_empty() {
            continue;
        }

        match line_to_evm_block(line) {
            Ok((parsed_block, height)) => {
                if height >= start_height {
                    last_height = last_height.max(height);
                    if !only_load_ranges {
                        new_blocks.push(parsed_block);
                    }
                    *last_line = line_idx;
                }

                match current_range {
                    Some((start, end)) if end + 1 == height => {
                        current_range = Some((start, height));
                    }
                    _ => {
                        if let Some((start, end)) = current_range.take() {
                            block_ranges.push(start..=end);
                        }
                        current_range = Some((height, height));
                    }
                }
            }
            Err(_) => {
                warn!("Failed to parse line: {}...", line.get(0..50).unwrap_or(line));
                continue;
            }
        }
    }

    if let Some((start, end)) = current_range {
        block_ranges.push(start..=end);
    }

    ScanResult {
        path: path.to_path_buf(),
        next_expected_height: last_height + 1,
        new_blocks,
        new_block_ranges: block_ranges,
    }
}

fn date_from_datetime(dt: OffsetDateTime) -> String {
    dt.format(&format_description!("[year][month][day]")).unwrap()
}

/// Block source that monitors the local ingest directory for the HL node.
#[derive(Debug, Clone)]
pub struct HlNodeBlockSource {
    pub fallback: BlockSourceBoxed,
    pub local_ingest_dir: PathBuf,
    pub local_blocks_cache: Arc<Mutex<LocalBlocksCache>>, // height → block
    pub last_local_fetch: Arc<Mutex<Option<(u64, OffsetDateTime)>>>, // for rate limiting requests to fallback
}

impl BlockSource for HlNodeBlockSource {
    fn collect_block(&self, height: u64) -> BoxFuture<eyre::Result<BlockAndReceipts>> {
        Box::pin(async move {
            let now = OffsetDateTime::now_utc();

            if let Some(block) = self.try_collect_local_block(height).await {
                self.update_last_fetch(height, now).await;
                return Ok(block);
            }

            if let Some((last_height, last_poll_time)) = *self.last_local_fetch.lock().await {
                let more_recent = last_height < height;
                let too_soon = now - last_poll_time < Self::MAX_ALLOWED_THRESHOLD_BEFORE_FALLBACK;
                if more_recent && too_soon {
                    return Err(eyre::eyre!(
                            "Not found locally; limiting polling rate before fallback so that hl-node has chance to catch up"
                        ));
                }
            }

            info!("Falling back to s3/ingest-dir for block @ Height [{height}]");
            let block = self.fallback.collect_block(height).await?;
            self.update_last_fetch(height, now).await;
            Ok(block)
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
            if let Some((_, height)) = read_last_complete_line(&mut file) {
                info!("Latest block number: {} with path {}", height, dir.display());
                Some(height)
            } else {
                warn!(
                    "Failed to parse the hl-node hourly file at {:?}; fallback to s3/ingest-dir",
                    file
                );
                self.fallback.find_latest_block_number().await
            }
        })
    }

    fn recommended_chunk_size(&self) -> u64 {
        self.fallback.recommended_chunk_size()
    }
}

fn read_last_complete_line<R: Read + Seek>(read: &mut R) -> Option<(BlockAndReceipts, u64)> {
    const CHUNK_SIZE: u64 = 50000;
    let mut buf = Vec::with_capacity(CHUNK_SIZE as usize);
    let mut pos = read.seek(SeekFrom::End(0)).unwrap();
    let mut last_line = Vec::new();

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
            if let Ok((evm_block, height)) =
                line_to_evm_block(str::from_utf8(candidate).unwrap())
            {
                return Some((evm_block, height));
            }
            // Incomplete line; truncate and continue
            last_line.truncate(idx);
        }

        if pos < read_size {
            break;
        }
        pos -= read_size;
    }

    line_to_evm_block(&String::from_utf8(last_line).unwrap()).ok()
}

impl HlNodeBlockSource {
    /// [HlNodeBlockSource] picks the faster one between local ingest directory and s3/ingest-dir.
    /// But if we immediately fallback to s3/ingest-dir, in case of S3, it may cause unnecessary
    /// requests to S3 while it'll return 404.
    ///
    /// To avoid unnecessary fallback, we set a short threshold period.
    /// This threshold is several times longer than the expected block time, reducing redundant fallback attempts.
    pub(crate) const MAX_ALLOWED_THRESHOLD_BEFORE_FALLBACK: Duration = Duration::milliseconds(5000);

    async fn update_last_fetch(&self, height: u64, now: OffsetDateTime) {
        let mut last_fetch = self.last_local_fetch.lock().await;
        if let Some((last_height, _)) = *last_fetch {
            if last_height >= height {
                return;
            }
        }
        *last_fetch = Some((height, now));
    }

    async fn try_collect_local_block(&self, height: u64) -> Option<BlockAndReceipts> {
        let mut u_cache = self.local_blocks_cache.lock().await;
        if let Some(block) = u_cache.cache.remove(&height) {
            return Some(block);
        }

        let path = u_cache.ranges.get(&height).cloned()?;

        info!("Loading block data from {:?}", path);
        u_cache.load_scan_result(scan_hour_file(
            &path,
            &mut 0,
            ScanOptions { start_height: 0, only_load_ranges: false },
        ));
        u_cache.cache.get(&height).cloned()
    }

    fn datetime_from_path(path: &Path) -> Option<OffsetDateTime> {
        let dt_part = path.parent()?.file_name()?.to_str()?;
        let hour_part = path.file_name()?.to_str()?;

        let hour: u8 = hour_part.parse().ok()?;
        Some(OffsetDateTime::new_utc(
            Date::parse(dt_part, &format_description!("[year][month][day]")).ok()?,
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
        cache: &Arc<Mutex<LocalBlocksCache>>,
        cutoff_height: u64,
    ) -> eyre::Result<()> {
        let mut u_cache = cache.lock().await;

        for subfile in Self::all_hourly_files(root).unwrap_or_default() {
            let mut file = File::open(&subfile).expect("Failed to open hour file path");

            if let Some((_, height)) = read_last_complete_line(&mut file) {
                if height < cutoff_height {
                    continue;
                }
            } else {
                warn!("Failed to parse last line of file, fallback to slow path: {:?}", subfile);
            }

            let mut scan_result = scan_hour_file(
                &subfile,
                &mut 0,
                ScanOptions { start_height: cutoff_height, only_load_ranges: true },
            );
            // Only store the block ranges for now; actual block data will be loaded lazily later to optimize memory usage
            scan_result.new_blocks.clear();
            u_cache.load_scan_result(scan_result);
        }

        if u_cache.ranges.is_empty() {
            warn!("No ranges found in {:?}", root);
        } else {
            let (min, _) = u_cache.ranges.first_range_value().unwrap();
            let (max, _) = u_cache.ranges.last_range_value().unwrap();
            info!(
                "Populated {} ranges (min: {}, max: {})",
                u_cache.ranges.len(),
                min.start(),
                max.end()
            );
        }

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
                    let scan_result = scan_hour_file(
                        &hour_file,
                        &mut last_line,
                        ScanOptions { start_height: next_height, only_load_ranges: false },
                    );
                    next_height = scan_result.next_expected_height;

                    let mut u_cache = cache.lock().await;
                    u_cache.load_scan_result(scan_result);
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
            local_blocks_cache: Arc::new(Mutex::new(LocalBlocksCache::new())),
            last_local_fetch: Arc::new(Mutex::new(None)),
        };
        block_source.run(next_block_number).await.unwrap();
        block_source
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::types::reth_compat;
    use crate::node::types::ReadPrecompileCalls;
    use crate::pseudo_peer::sources::LocalBlockSource;
    use alloy_consensus::{BlockBody, Header};
    use alloy_primitives::{Address, Bloom, Bytes, B256, B64, U256};
    use std::io::Write;
    use std::time::Duration;

    #[test]
    fn test_datetime_from_path() {
        let path = Path::new("/home/username/hl/data/evm_block_and_receipts/hourly/20250731/4");
        let dt = HlNodeBlockSource::datetime_from_path(path).unwrap();
        println!("{dt:?}");
    }

    #[tokio::test]
    async fn test_backfill() {
        let test_path = Path::new("/root/evm_block_and_receipts");
        if !test_path.exists() {
            return;
        }

        let cache = Arc::new(Mutex::new(LocalBlocksCache::new()));
        HlNodeBlockSource::try_backfill_local_blocks(test_path, &cache, 1000000).await.unwrap();

        let u_cache = cache.lock().await;
        println!("{:?}", u_cache.ranges);
        assert_eq!(
            u_cache.ranges.get(&9735058),
            Some(&test_path.join(HOURLY_SUBDIR).join("20250729").join("22"))
        );
    }

    fn scan_result_from_single_block(block: BlockAndReceipts) -> ScanResult {
        let height = match &block.block {
            EvmBlock::Reth115(b) => b.header.header.number,
        };
        ScanResult {
            path: PathBuf::from("/nonexistent-block"),
            next_expected_height: height + 1,
            new_blocks: vec![block],
            new_block_ranges: vec![height..=height],
        }
    }

    fn empty_block(
        number: u64,
        timestamp: u64,
        extra_data: &'static [u8],
    ) -> LocalBlockAndReceipts {
        let extra_data = Bytes::from_static(extra_data);
        let res = BlockAndReceipts {
            block: EvmBlock::Reth115(reth_compat::SealedBlock {
                header: reth_compat::SealedHeader {
                    header: Header {
                        parent_hash: B256::ZERO,
                        ommers_hash: B256::ZERO,
                        beneficiary: Address::ZERO,
                        state_root: B256::ZERO,
                        transactions_root: B256::ZERO,
                        receipts_root: B256::ZERO,
                        logs_bloom: Bloom::ZERO,
                        difficulty: U256::ZERO,
                        number,
                        gas_limit: 0,
                        gas_used: 0,
                        timestamp,
                        extra_data,
                        mix_hash: B256::ZERO,
                        nonce: B64::ZERO,
                        base_fee_per_gas: None,
                        withdrawals_root: None,
                        blob_gas_used: None,
                        excess_blob_gas: None,
                        parent_beacon_block_root: None,
                        requests_hash: None,
                    },
                    hash: B256::ZERO,
                },
                body: BlockBody { transactions: vec![], ommers: vec![], withdrawals: None },
            }),
            receipts: vec![],
            system_txs: vec![],
            read_precompile_calls: ReadPrecompileCalls(vec![]),
            highest_precompile_address: None,
        };
        LocalBlockAndReceipts(timestamp.to_string(), res)
    }

    fn setup_temp_dir_and_file() -> eyre::Result<(tempfile::TempDir, File)> {
        let now = OffsetDateTime::now_utc();
        let day_str = date_from_datetime(now);
        let hour = now.hour();

        let temp_dir = tempfile::tempdir()?;
        let path = temp_dir.path().join(HOURLY_SUBDIR).join(&day_str).join(format!("{hour}"));
        std::fs::create_dir_all(path.parent().unwrap())?;

        Ok((temp_dir, File::create(path)?))
    }

    struct BlockSourceHierarchy {
        block_source: HlNodeBlockSource,
        _temp_dir: tempfile::TempDir,
        file1: File,
        current_block: LocalBlockAndReceipts,
        future_block_hl_node: LocalBlockAndReceipts,
        future_block_fallback: LocalBlockAndReceipts,
    }

    async fn setup_block_source_hierarchy() -> eyre::Result<BlockSourceHierarchy> {
        // Setup fallback block source
        let block_source_fallback = HlNodeBlockSource::new(
            BlockSourceBoxed::new(Box::new(LocalBlockSource::new("/nonexistent"))),
            PathBuf::from("/nonexistent"),
            1000000,
        )
        .await;
        let block_hl_node_0 = empty_block(1000000, 1722633600, b"hl-node");
        let block_hl_node_1 = empty_block(1000001, 1722633600, b"hl-node");
        let block_fallback_1 = empty_block(1000001, 1722633600, b"fallback");

        let (temp_dir1, mut file1) = setup_temp_dir_and_file()?;
        writeln!(&mut file1, "{}", serde_json::to_string(&block_hl_node_0)?)?;

        let block_source = HlNodeBlockSource::new(
            BlockSourceBoxed::new(Box::new(block_source_fallback.clone())),
            temp_dir1.path().to_path_buf(),
            1000000,
        )
        .await;

        block_source_fallback
            .local_blocks_cache
            .lock()
            .await
            .load_scan_result(scan_result_from_single_block(block_fallback_1.1.clone()));

        Ok(BlockSourceHierarchy {
            block_source,
            _temp_dir: temp_dir1,
            file1,
            current_block: block_hl_node_0,
            future_block_hl_node: block_hl_node_1,
            future_block_fallback: block_fallback_1,
        })
    }

    #[tokio::test]
    async fn test_update_last_fetch_no_fallback() -> eyre::Result<()> {
        let hierarchy = setup_block_source_hierarchy().await?;
        let BlockSourceHierarchy {
            block_source,
            current_block,
            future_block_hl_node,
            mut file1,
            ..
        } = hierarchy;

        let block = block_source.collect_block(1000000).await.unwrap();
        assert_eq!(block, current_block.1);

        let block = block_source.collect_block(1000001).await;
        assert!(block.is_err());

        writeln!(&mut file1, "{}", serde_json::to_string(&future_block_hl_node)?)?;
        tokio::time::sleep(Duration::from_millis(100)).await;

        let block = block_source.collect_block(1000001).await.unwrap();
        assert_eq!(block, future_block_hl_node.1);

        Ok(())
    }

    #[tokio::test]
    async fn test_update_last_fetch_fallback() -> eyre::Result<()> {
        let hierarchy = setup_block_source_hierarchy().await?;
        let BlockSourceHierarchy {
            block_source,
            current_block,
            future_block_fallback,
            mut file1,
            ..
        } = hierarchy;

        let block = block_source.collect_block(1000000).await.unwrap();
        assert_eq!(block, current_block.1);

        tokio::time::sleep(HlNodeBlockSource::MAX_ALLOWED_THRESHOLD_BEFORE_FALLBACK.unsigned_abs())
            .await;

        writeln!(&mut file1, "{}", serde_json::to_string(&future_block_fallback)?)?;
        let block = block_source.collect_block(1000001).await.unwrap();
        assert_eq!(block, future_block_fallback.1);

        Ok(())
    }
}
