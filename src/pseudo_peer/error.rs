use thiserror::Error;

#[derive(Error, Debug)]
pub enum PseudoPeerError {
    #[error("Block source error: {0}")]
    BlockSource(String),

    #[error("Network error: {0}")]
    Network(#[from] reth_network::error::NetworkError),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("AWS S3 error: {0}")]
    S3(#[from] aws_sdk_s3::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] rmp_serde::encode::Error),

    #[error("Deserialization error: {0}")]
    Deserialization(#[from] rmp_serde::decode::Error),

    #[error("Compression error: {0}")]
    Compression(String),
}

impl From<eyre::Error> for PseudoPeerError {
    fn from(err: eyre::Error) -> Self {
        PseudoPeerError::Config(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, PseudoPeerError>;
