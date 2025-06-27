use clap::Parser;
use reth_my_p2p::cli::PseudoPeerCommand;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let cli = PseudoPeerCommand::parse();
    cli.logs.init_tracing()?;

    // Parse and create block source configuration
    let block_source_config = cli.source.parse().await?;
    let block_source = block_source_config.create_cached_block_source().await;

    // Start the worker
    reth_my_p2p::start_pseudo_peer(cli.destination_peer, block_source).await?;
    Ok(())
}
