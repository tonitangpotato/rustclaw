#![allow(dead_code)]

mod agent;
mod auth_profiles;
mod browser;
mod channels;
mod config;
mod credential;
mod cron;
mod daemon;
mod dashboard;
mod distributed;
mod engram_hooks;
mod export;
mod heartbeat;
mod hooks;
mod llm;
mod memory;
mod oauth;
mod orchestrator;
mod plugins;
mod reload;
mod sandbox;
mod safety;
mod search;
mod serverless;
mod session;
mod skills;
mod stt;
mod text_utils;
mod tools;
mod tts;
mod user_model;
mod worktree;
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
    /// Manage the RustClaw daemon service
    #[command(subcommand)]
    Daemon(DaemonCommands),
}

#[derive(clap::Subcommand, Debug)]
enum DaemonCommands {
    /// Start the daemon (registers and loads launchd service)
    Start {
        /// Path to config file
        #[arg(short, long, default_value = "rustclaw.yaml")]
        config: String,
        /// Workspace directory
        #[arg(short, long)]
        workspace: Option<String>,
    },
    /// Stop the daemon
    Stop,
    /// Show daemon status
    Status,
    /// Restart the daemon
    Restart {
        /// Path to config file
        #[arg(short, long, default_value = "rustclaw.yaml")]
        config: String,
        /// Workspace directory
        #[arg(short, long)]
        workspace: Option<String>,
    },
    /// View daemon logs
    Logs {
        /// Follow log output (like tail -f)
        #[arg(short, long)]
        follow: bool,
        /// Number of lines to show
        #[arg(short = 'n', long, default_value = "50")]
        lines: usize,
    },
    /// Install the daemon service (without starting)
    Install {
        /// Path to config file
        #[arg(short, long, default_value = "rustclaw.yaml")]
        config: String,
        /// Workspace directory
        #[arg(short, long)]
        workspace: Option<String>,
    },
    /// Uninstall the daemon service
    Uninstall,
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

            // Initialize memory (wrap in Arc for tool sharing)
            let mem = std::sync::Arc::new(memory::MemoryManager::new(&cfg, &workspace_dir).await?);
            tracing::info!("Memory initialized");

            // Initialize hooks with safety checks
            let mut hook_registry = hooks::HookRegistry::new();
            hook_registry.register(Box::new(safety::PromptInjectionHook));
            hook_registry.register(Box::new(safety::SensitiveLeakHook));

            // Register Engram memory hooks (auto-recall and auto-store)
            if cfg.memory.auto_recall {
                hook_registry.register(Box::new(engram_hooks::EngramRecallHook::new(mem.clone())));
                tracing::info!("Engram auto-recall hook enabled");
            }
            if cfg.memory.auto_store {
                hook_registry.register(Box::new(engram_hooks::EngramStoreHook::new(mem.clone())));
                tracing::info!("Engram auto-store hook enabled");
            }
            tracing::info!("Hook system ready ({} hooks)", hook_registry.count());

            // Initialize session manager
            let sessions = session::SessionManager::new(&cfg).await?;
            tracing::info!("Session manager ready");

            // Initialize orchestrator (if enabled)
            let orch = if cfg.orchestrator.enabled {
                tracing::info!(
                    "Orchestrator enabled: {} specialist(s), tick interval {}s",
                    cfg.orchestrator.specialists.len(),
                    cfg.orchestrator.tick_interval
                );
                Some(orchestrator::create_orchestrator(cfg.orchestrator.clone()))
            } else {
                tracing::info!("Orchestrator disabled");
                None
            };

            // Create shared runner handle for spawn_specialist tool (late-binding)
            let runner_handle: tools::SharedAgentRunner = 
                std::sync::Arc::new(tokio::sync::RwLock::new(None));

            // Initialize tools with memory and orchestrator access
            let mut tools = if let Some(ref orch_ref) = orch {
                tools::ToolRegistry::with_defaults_and_orchestrator(
                    &workspace_dir,
                    mem.clone(),
                    orch_ref.clone(),
                )
                .with_spawn_specialist(runner_handle.clone(), Some(orch_ref.clone()))
            } else {
                tools::ToolRegistry::with_defaults_and_memory(&workspace_dir, mem.clone())
                    .with_spawn_specialist(runner_handle.clone(), None)
            };

            // Register GID tools if enabled
            if cfg.gid.enabled {
                let graph_path = if std::path::Path::new(&cfg.gid.graph_path).is_absolute() {
                    cfg.gid.graph_path.clone()
                } else {
                    format!("{}/{}", workspace_dir, cfg.gid.graph_path)
                };
                // Ensure parent directory exists
                if let Some(parent) = std::path::Path::new(&graph_path).parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                tools = tools.with_gid(&graph_path);
                tracing::info!("GID tools enabled (graph: {})", graph_path);
            }

            tracing::info!("Tools registered: {}", tools.definitions().len());

            // Initialize plugin registry
            let mut plugin_registry = plugins::PluginRegistry::new();
            // Register built-in plugins here if needed
            // plugin_registry.register(Box::new(plugins::ExamplePlugin));

            // Load plugins
            let plugin_ctx = plugins::PluginContext::new(
                &workspace_dir,
                std::sync::Arc::new(cfg.clone()),
                mem.clone(),
            );
            if let Err(e) = plugin_registry.load_all(&plugin_ctx).await {
                tracing::error!("Failed to load plugins: {}", e);
            } else {
                tracing::info!("Plugin system ready ({} plugins)", plugin_registry.count());
            }

            // Clone memory Arc for auto-consolidation background task
            let mem_for_consolidation = mem.clone();

            // Log embedding status at startup
            tracing::info!("Embedding status: {}", mem.embedding_status());

            // Build agent runner
            let runner = agent::AgentRunner::new(cfg.clone(), ws, mem, sessions, hook_registry, tools);

            // Start channels (wraps runner in Arc)
            let runner = std::sync::Arc::new(runner);

            // Set the runner handle for spawn_specialist tool (late-binding)
            {
                let mut handle = runner_handle.write().await;
                *handle = Some(runner.clone());
            }
            tracing::info!("spawn_specialist tool ready");

            // Start config hot-reload watcher
            let (config_tx, config_rx, _watcher) =
                reload::start_config_watcher(&config, cfg.clone())?;
            reload::start_sighup_listener(config.clone(), config_tx.clone()).await;

            // Wire config changes to agent runner (hot-reload model, etc.)
            runner.start_config_reload_listener(config_rx);

            // Start heartbeat
            heartbeat::start_heartbeat(
                runner.clone(),
                cfg.heartbeat_interval,
                "heartbeat:main",
            )
            .await?;

            // Start cron jobs
            let cron_jobs = cron::parse_cron_jobs(&cfg.cron);
            if !cron_jobs.is_empty() {
                tracing::info!(
                    "Starting {} cron job(s) (timezone: {})...",
                    cron_jobs.len(),
                    cfg.cron.timezone
                );
                cron::start_cron(cron_jobs, runner.clone()).await?;
            }

            // Start orchestrator tick loop (if enabled)
            if let Some(ref orch_ref) = orch {
                let tick_interval = cfg.orchestrator.tick_interval;
                let orch_clone = orch_ref.clone();
                let runner_clone = runner.clone();
                tokio::spawn(async move {
                    orchestrator::start_orchestrator_loop(orch_clone, runner_clone, tick_interval).await;
                });
            }

            // Start auto-consolidation background task (every 6 hours)
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(6 * 3600));
                loop {
                    interval.tick().await;
                    match mem_for_consolidation.consolidate() {
                        Ok(()) => tracing::info!("Engram auto-consolidation completed"),
                        Err(e) => tracing::warn!("Engram auto-consolidation failed: {}", e),
                    }
                }
            });
            tracing::info!("Engram auto-consolidation scheduled (every 6 hours)");

            // Start web dashboard (if enabled)
            dashboard::start_dashboard(cfg.dashboard.clone(), cfg.clone(), runner.clone()).await?;

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
        Commands::Daemon(cmd) => {
            match cmd {
                DaemonCommands::Start { config, workspace } => {
                    daemon::daemon_start(&config, workspace.as_deref())?;
                }
                DaemonCommands::Stop => {
                    daemon::daemon_stop()?;
                }
                DaemonCommands::Status => {
                    daemon::daemon_status()?;
                }
                DaemonCommands::Restart { config, workspace } => {
                    daemon::daemon_restart(&config, workspace.as_deref())?;
                }
                DaemonCommands::Logs { follow, lines } => {
                    daemon::daemon_logs(follow, lines)?;
                }
                DaemonCommands::Install { config, workspace } => {
                    daemon::daemon_install(&config, workspace.as_deref())?;
                }
                DaemonCommands::Uninstall => {
                    daemon::daemon_uninstall()?;
                }
            }
        }
    }

    Ok(())
}
