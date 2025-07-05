use clap::Parser;
use reth::builder::NodeHandle;
use reth_hl::{
    call_forwarder::{self, CallForwarderApiServer},
    chainspec::parser::HlChainSpecParser,
    hl_node_compliance::install_hl_node_compliance,
    node::{
        cli::{Cli, HlNodeArgs},
        storage::tables::Tables,
        HlNode,
    },
    tx_forwarder::{self, EthForwarderApiServer},
};
use tracing::info;

// We use jemalloc for performance reasons
#[cfg(all(feature = "jemalloc", unix))]
#[global_allocator]
static ALLOC: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

fn main() -> eyre::Result<()> {
    reth_cli_util::sigsegv_handler::install();

    // Enable backtraces unless a RUST_BACKTRACE value has already been explicitly provided.
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        std::env::set_var("RUST_BACKTRACE", "1");
    }

    Cli::<HlChainSpecParser, HlNodeArgs>::parse().run(|builder, ext| async move {
        let default_upstream_rpc_url = builder.config().chain.official_rpc_url();
        builder.builder.database.create_tables_for::<Tables>()?;

        let (node, engine_handle_tx) =
            HlNode::new(ext.block_source_args.parse().await?, ext.hl_node_compliant);
        let NodeHandle { node, node_exit_future: exit_future } = builder
            .node(node)
            .extend_rpc_modules(move |ctx| {
                let upstream_rpc_url =
                    ext.upstream_rpc_url.unwrap_or_else(|| default_upstream_rpc_url.to_owned());

                ctx.modules.replace_configured(
                    tx_forwarder::EthForwarderExt::new(upstream_rpc_url.clone()).into_rpc(),
                )?;
                info!("Transaction will be forwarded to {}", upstream_rpc_url);

                if ext.forward_call {
                    ctx.modules.replace_configured(
                        call_forwarder::CallForwarderExt::new(upstream_rpc_url.clone()).into_rpc(),
                    )?;
                    info!("Call/gas estimation will be forwarded to {}", upstream_rpc_url);
                }

                if ext.hl_node_compliant {
                    install_hl_node_compliance(ctx)?;
                    info!("hl-node compliant mode enabled");
                }

                Ok(())
            })
            .launch()
            .await?;

        engine_handle_tx.send(node.beacon_engine_handle.clone()).unwrap();

        exit_future.await
    })?;
    Ok(())
}
