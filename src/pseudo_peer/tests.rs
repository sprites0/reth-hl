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
