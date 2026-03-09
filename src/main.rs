#![allow(dead_code)]

mod agent;
mod channels;
mod config;
mod heartbeat;
mod hooks;
mod llm;
mod memory;
mod session;
mod tools;
mod tts;
mod workspace;

use clap::Parser;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "rustclaw", version, about = "Rust-native AI agent framework")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand, Debug)]
enum Commands {
    /// Start the agent gateway (Telegram, etc.)
    Run {
        /// Path to config file
        #[arg(short, long, default_value = "rustclaw.yaml")]
        config: String,

        /// Workspace directory
        #[arg(short, long)]
        workspace: Option<String>,
    },
    /// Interactive CLI mode
    Chat {
        #[arg(short, long, default_value = "rustclaw.yaml")]
        config: String,
    },
    /// Show current configuration
    Config {
        #[arg(short, long, default_value = "rustclaw.yaml")]
        config: String,
    },
    /// Run setup wizard
    Setup,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Run { config, workspace } => {
            tracing::info!("Starting RustClaw gateway...");
            let cfg = config::load_config(&config)?;
            let workspace_dir = workspace
                .or(cfg.workspace.clone())
                .unwrap_or_else(|| ".".to_string());

            let ws = workspace::Workspace::load(&workspace_dir)?;
            tracing::info!("Workspace loaded: {}", workspace_dir);
            tracing::info!("Agent: {}", ws.identity_name().unwrap_or("unnamed"));

            // Initialize memory
            let mem = memory::MemoryManager::new(&cfg, &workspace_dir).await?;
            tracing::info!("Memory initialized");

            // Initialize hooks
            let hook_registry = hooks::HookRegistry::new();
            tracing::info!("Hook system ready ({} hooks)", hook_registry.count());

            // Initialize session manager
            let sessions = session::SessionManager::new(&cfg).await?;
            tracing::info!("Session manager ready");

            // Initialize tools
            let tools = tools::ToolRegistry::with_defaults(&workspace_dir);
            tracing::info!("Tools registered: {}", tools.definitions().len());

            // Build agent runner
            let runner = agent::AgentRunner::new(cfg.clone(), ws, mem, sessions, hook_registry, tools);

            // Start channels (wraps runner in Arc)
            let runner = std::sync::Arc::new(runner);

            // Start heartbeat
            heartbeat::start_heartbeat(
                runner.clone(),
                cfg.heartbeat_interval,
                "heartbeat:main",
            )
            .await?;

            // Start channels
            channels::start_gateway(cfg, runner).await?;
        }
        Commands::Chat { config } => {
            tracing::info!("Interactive chat mode (not yet implemented)");
            let _cfg = config::load_config(&config)?;
            // TODO: Interactive CLI
        }
        Commands::Config { config } => {
            let cfg = config::load_config(&config)?;
            println!("{}", serde_yaml::to_string(&cfg)?);
        }
        Commands::Setup => {
            tracing::info!("Setup wizard (not yet implemented)");
            // TODO: Interactive setup
        }
    }

    Ok(())
}
