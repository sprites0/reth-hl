use super::config::BlockSourceConfig;
use clap::{Args, Parser};
use reth_node_core::args::LogArgs;

#[derive(Debug, Clone, Args)]
pub struct BlockSourceArgs {
    /// Block source to use for the benchmark.
    /// Example: s3://hl-mainnet-evm-blocks
    /// Example: /home/user/personal/evm-blocks
    ///
    /// For S3, you can use environment variables like AWS_PROFILE, etc.
    #[arg(long)]
    block_source: Option<String>,

    #[arg(long)]
    block_source_from_node: Option<String>,

    /// Shorthand of --block-source=s3://hl-mainnet-evm-blocks
    #[arg(long = "s3", default_value_t = false)]
    s3: bool,
}

impl BlockSourceArgs {
    pub async fn parse(&self) -> eyre::Result<BlockSourceConfig> {
        if self.s3 {
            return Ok(BlockSourceConfig::s3_default().await);
        }

        let Some(value) = self.block_source.as_ref() else {
            return Err(eyre::eyre!(
                "You need to specify a block source e.g., --s3 or --block-source=/path/to/blocks"
            ));
        };

        let mut config = if let Some(bucket) = value.strip_prefix("s3://") {
            BlockSourceConfig::s3(bucket.to_string()).await
        } else {
            BlockSourceConfig::local(value.to_string())
        };

        if let Some(block_source_from_node) = self.block_source_from_node.as_ref() {
            config = config.with_block_source_from_node(block_source_from_node.to_string());
        }

        Ok(config)
    }
}

#[derive(Debug, Parser)]
pub struct PseudoPeerCommand {
    #[command(flatten)]
    pub logs: LogArgs,

    #[command(flatten)]
    pub source: BlockSourceArgs,

    /// Destination peer to connect to.
    /// Example: enode://412...1a@0.0.0.0:30304
    #[arg(long)]
    pub destination_peer: String,
}
