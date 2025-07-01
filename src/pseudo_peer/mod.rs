//! A pseudo peer library that ingests multiple block sources to reth
//!
//! This library exposes `start_pseudo_peer` to support reth-side NetworkState/StateFetcher
//! to fetch blocks and feed it to its stages

pub mod cli;
pub mod config;
pub mod consts;
pub mod error;
pub mod network;
pub mod service;
pub mod sources;
pub mod utils;

pub use cli::*;
pub use config::*;
pub use error::*;
pub use network::*;
pub use service::*;
pub use sources::*;

#[cfg(test)]
mod tests;

use tokio::sync::mpsc;
use tracing::info;

/// Re-export commonly used types
pub mod prelude {
    pub use super::{
        config::BlockSourceConfig,
        error::{PseudoPeerError, Result},
        service::{BlockPoller, PseudoPeer},
        sources::{BlockSource, CachedBlockSource, LocalBlockSource, S3BlockSource},
    };
}

use reth_network::{NetworkEvent, NetworkEventListenerProvider};

/// Main function that starts the network manager and processes eth requests
pub async fn start_pseudo_peer(
    destination_peer: String,
    block_source: BlockSourceBoxed,
) -> eyre::Result<()> {
    let blockhash_cache = new_blockhash_cache();

    // Create network manager
    let (mut network, start_tx) = create_network_manager::<BlockSourceBoxed>(
        destination_peer,
        block_source.clone(),
        blockhash_cache.clone(),
    )
    .await?;

    // Create the channels for receiving eth messages
    let (eth_tx, mut eth_rx) = mpsc::channel(32);
    let (transaction_tx, mut transaction_rx) = mpsc::unbounded_channel();

    network.set_eth_request_handler(eth_tx);
    network.set_transactions(transaction_tx);

    let network_handle = network.handle().clone();
    let mut network_events = network_handle.event_listener();
    info!("Starting network manager...");

    let mut service = PseudoPeer::new(block_source, blockhash_cache.clone());
    tokio::spawn(network);
    let mut first = true;

    // Main event loop
    loop {
        tokio::select! {
            Some(event) = tokio_stream::StreamExt::next(&mut network_events) => {
                info!("Network event: {event:?}");
                if matches!(event, NetworkEvent::ActivePeerSession { .. }) && first {
                    start_tx.send(()).await?;
                    first = false;
                }
            }

            _ = transaction_rx.recv() => {}

            Some(eth_req) = eth_rx.recv() => {
                service.process_eth_request(eth_req).await?;
                info!("Processed eth request");
            }
        }
    }
}
