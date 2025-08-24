use crate::node::types::{BlockAndReceipts, EvmBlock};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{BufRead, BufReader},
    ops::RangeInclusive,
    path::{Path, PathBuf},
};
use tracing::warn;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LocalBlockAndReceipts(pub String, pub BlockAndReceipts);

pub struct ScanResult {
    pub path: PathBuf,
    pub next_expected_height: u64,
    pub new_blocks: Vec<BlockAndReceipts>,
    pub new_block_ranges: Vec<RangeInclusive<u64>>,
}

pub struct ScanOptions {
    pub start_height: u64,
    pub only_load_ranges: bool,
}

pub struct Scanner;

impl Scanner {
    pub fn line_to_evm_block(line: &str) -> serde_json::Result<(BlockAndReceipts, u64)> {
        let LocalBlockAndReceipts(_, parsed_block): LocalBlockAndReceipts =
            serde_json::from_str(line)?;
        let height = match &parsed_block.block {
            EvmBlock::Reth115(b) => b.header.header.number,
        };
        Ok((parsed_block, height))
    }

    pub fn scan_hour_file(path: &Path, last_line: &mut usize, options: ScanOptions) -> ScanResult {
        let lines: Vec<String> =
            BufReader::new(File::open(path).expect("Failed to open hour file"))
                .lines()
                .collect::<Result<_, _>>()
                .unwrap();
        let skip = if *last_line == 0 { 0 } else { *last_line - 1 };
        let mut new_blocks = Vec::new();
        let mut last_height = options.start_height;
        let mut block_ranges = Vec::new();
        let mut current_range: Option<(u64, u64)> = None;

        for (line_idx, line) in lines.iter().enumerate().skip(skip) {
            if line_idx < *last_line || line.trim().is_empty() {
                continue;
            }

            match Self::line_to_evm_block(line) {
                Ok((parsed_block, height)) if height >= options.start_height => {
                    last_height = last_height.max(height);
                    if !options.only_load_ranges {
                        new_blocks.push(parsed_block);
                    }
                    *last_line = line_idx;

                    match current_range {
                        Some((start, end)) if end + 1 == height => {
                            current_range = Some((start, height))
                        }
                        _ => {
                            if let Some((start, end)) = current_range.take() {
                                block_ranges.push(start..=end);
                            }
                            current_range = Some((height, height));
                        }
                    }
                }
                Ok(_) => {}
                Err(_) => warn!("Failed to parse line: {}...", line.get(0..50).unwrap_or(line)),
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
}
