use std::path::Path;

use crate::pseudo_peer::{prelude::*, BlockSourceType};

#[tokio::test]
async fn test_block_source_config_s3() {
    let config = BlockSourceConfig::s3("test-bucket".to_string()).await;
    assert!(
        matches!(config.source_type, BlockSourceType::S3 { bucket } if bucket == "test-bucket")
    );
}

#[tokio::test]
async fn test_block_source_config_local() {
    let config = BlockSourceConfig::local("/test/path".into());
    assert!(
        matches!(config.source_type, BlockSourceType::Local { path } if path == Path::new("/test/path"))
    );
}

#[test]
fn test_error_types() {
    let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "File not found");
    let benchmark_error: PseudoPeerError = io_error.into();

    match benchmark_error {
        PseudoPeerError::Io(_) => (),
        _ => panic!("Expected Io error"),
    }
}
