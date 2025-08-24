/// Shared utilities for block sources

/// Finds the file/directory with the largest number in its name from a list of files
pub fn name_with_largest_number(files: &[String], is_dir: bool) -> Option<(u64, String)> {
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
    files.last().cloned()
}

/// Generates the RMP file path for a given block height
pub fn rmp_path(height: u64) -> String {
    let f = ((height - 1) / 1_000_000) * 1_000_000;
    let s = ((height - 1) / 1_000) * 1_000;
    format!("{f}/{s}/{height}.rmp.lz4")
}
