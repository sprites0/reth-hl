use crate::{
    chainspec::{parser::HlChainSpecParser, HlChainSpec},
    node::{
        consensus::HlConsensus, evm::config::HlEvmConfig, network::HlNetworkPrimitives, HlNode,
    },
    pseudo_peer::BlockSourceArgs,
};
use clap::{Args, Parser};
use reth::{
    args::LogArgs,
    builder::{NodeBuilder, WithLaunchContext},
    cli::Commands,
    prometheus_exporter::install_prometheus_recorder,
    version::{LONG_VERSION, SHORT_VERSION},
    CliRunner,
};
use reth_chainspec::EthChainSpec;
use reth_cli::chainspec::ChainSpecParser;
use reth_cli_commands::launcher::FnLauncher;
use reth_db::DatabaseEnv;
use reth_tracing::FileWorkerGuard;
use std::{
    fmt::{self},
    future::Future,
    sync::Arc,
};
use tracing::info;

#[derive(Debug, Clone, Args)]
#[non_exhaustive]
pub struct HlNodeArgs {
    #[command(flatten)]
    pub block_source_args: BlockSourceArgs,

    /// Upstream RPC URL to forward incoming transactions.
    ///
    /// Default to Hyperliquid's RPC URL when not provided (https://rpc.hyperliquid.xyz/evm).
    #[arg(long, env = "UPSTREAM_RPC_URL")]
    pub upstream_rpc_url: Option<String>,

    /// Enable hl-node compliant mode.
    ///
    /// This option
    /// 1. filters out system transactions from block transaction list.
    /// 2. filters out logs that are not from the block's transactions.
    /// 3. filters out logs and transactions from subscription.
    #[arg(long, env = "HL_NODE_COMPLIANT")]
    pub hl_node_compliant: bool,

    /// Forward eth_call and eth_estimateGas to the upstream RPC.
    ///
    /// This is useful when read precompile is needed for gas estimation.
    #[arg(long, env = "FORWARD_CALL")]
    pub forward_call: bool,
}

/// The main reth_hl cli interface.
///
/// This is the entrypoint to the executable.
#[derive(Debug, Parser)]
#[command(author, version = SHORT_VERSION, long_version = LONG_VERSION, about = "Reth", long_about = None)]
pub struct Cli<Spec: ChainSpecParser = HlChainSpecParser, Ext: clap::Args + fmt::Debug = HlNodeArgs>
{
    /// The command to run
    #[command(subcommand)]
    pub command: Commands<Spec, Ext>,

    #[command(flatten)]
    logs: LogArgs,
}

impl<C, Ext> Cli<C, Ext>
where
    C: ChainSpecParser<ChainSpec = HlChainSpec>,
    Ext: clap::Args + fmt::Debug,
{
    /// Execute the configured cli command.
    ///
    /// This accepts a closure that is used to launch the node via the
    /// [`NodeCommand`](reth_cli_commands::node::NodeCommand).
    pub fn run<L, Fut>(self, launcher: L) -> eyre::Result<()>
    where
        L: FnOnce(WithLaunchContext<NodeBuilder<Arc<DatabaseEnv>, C::ChainSpec>>, Ext) -> Fut,
        Fut: Future<Output = eyre::Result<()>>,
    {
        self.with_runner(CliRunner::try_default_runtime()?, launcher)
    }

    /// Execute the configured cli command with the provided [`CliRunner`].
    pub fn with_runner<L, Fut>(mut self, runner: CliRunner, launcher: L) -> eyre::Result<()>
    where
        L: FnOnce(WithLaunchContext<NodeBuilder<Arc<DatabaseEnv>, C::ChainSpec>>, Ext) -> Fut,
        Fut: Future<Output = eyre::Result<()>>,
    {
        // Add network name if available to the logs dir
        if let Some(chain_spec) = self.command.chain_spec() {
            self.logs.log_file_directory =
                self.logs.log_file_directory.join(chain_spec.chain().to_string());
        }

        let _guard = self.init_tracing()?;
        info!(target: "reth::cli", "Initialized tracing, debug log directory: {}", self.logs.log_file_directory);

        // Install the prometheus recorder to be sure to record all metrics
        let _ = install_prometheus_recorder();

        let components =
            |spec: Arc<C::ChainSpec>| (HlEvmConfig::new(spec.clone()), HlConsensus::new(spec));

        match self.command {
            Commands::Node(command) => runner.run_command_until_exit(|ctx| {
                command.execute(ctx, FnLauncher::new::<C, Ext>(launcher))
            }),
            Commands::Init(command) => {
                runner.run_blocking_until_ctrl_c(command.execute::<HlNode>())
            }
            Commands::InitState(command) => {
                runner.run_blocking_until_ctrl_c(command.execute::<HlNode>())
            }
            Commands::DumpGenesis(command) => runner.run_blocking_until_ctrl_c(command.execute()),
            Commands::Db(command) => runner.run_blocking_until_ctrl_c(command.execute::<HlNode>()),
            Commands::Stage(command) => runner.run_command_until_exit(|ctx| {
                command.execute::<HlNode, _, _, HlNetworkPrimitives>(ctx, components)
            }),
            Commands::P2P(command) => {
                runner.run_until_ctrl_c(command.execute::<HlNetworkPrimitives>())
            }
            Commands::Config(command) => runner.run_until_ctrl_c(command.execute()),
            Commands::Recover(command) => {
                runner.run_command_until_exit(|ctx| command.execute::<HlNode>(ctx))
            }
            Commands::Prune(command) => runner.run_until_ctrl_c(command.execute::<HlNode>()),
            Commands::Import(command) => {
                runner.run_blocking_until_ctrl_c(command.execute::<HlNode, _, _>(components))
            }
            Commands::Debug(_command) => todo!(),
            #[cfg(feature = "dev")]
            Commands::TestVectors(_command) => todo!(),
            Commands::ImportEra(_command) => {
                todo!()
            }
            Commands::Download(_command) => {
                todo!()
            }
        }
    }

    /// Initializes tracing with the configured options.
    ///
    /// If file logging is enabled, this function returns a guard that must be kept alive to ensure
    /// that all logs are flushed to disk.
    pub fn init_tracing(&self) -> eyre::Result<Option<FileWorkerGuard>> {
        let guard = self.logs.init_tracing()?;
        Ok(guard)
    }
}
