#![allow(dead_code)]

mod agent;
mod autopilot;
mod auth_profiles;
mod browser;
mod channels;
mod claude_cli;
mod config;
mod context;
mod credential;
mod cron;
mod daemon;
mod dashboard;
mod distributed;
mod engram_hooks;
mod gid_storage;
mod heartbeat;
mod hooks;
mod llm;
mod markdown;
mod interoceptive;
mod memory;
mod oauth;
mod orchestrator;
mod plugins;
mod prompt;
mod reload;
mod sandbox;
mod safety;
mod search;
mod serverless;
mod session;
mod tool_result_storage;mod skills;
mod events;mod stt;
mod message_queue;
mod text_utils;
mod ritual_adapter;
mod ritual_runner;
mod tools;
pub mod tool_stats;
mod tts;
mod user_model;
mod voice_emotion;
mod voice_mode;
mod worktree;
// mod platform; // WIP: disabled until compilation fixed
mod workspace;

use clap::Parser;
use tracing_subscriber::EnvFilter;

/// Initialize tracing with file + stderr dual output.
/// File logging goes to ~/.rustclaw/logs/ with daily rotation.
/// Controlled by RUST_LOG env var or config (loaded later, so we use defaults here).
fn init_logging() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    // Determine log directory
    let log_dir = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".rustclaw/logs");

    // Ensure log directory exists
    let _ = std::fs::create_dir_all(&log_dir);

    // File appender with daily rotation
    let file_appender = tracing_appender::rolling::daily(&log_dir, "rustclaw.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // Leak the guard so it lives for the process lifetime
    // (dropping it would flush and close the file writer)
    std::mem::forget(_guard);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
        )
        .init();

    tracing::info!("Logging initialized — file output: {}/rustclaw.log", log_dir.display());
}

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
    // Panic hook — log panics before process dies
    std::panic::set_hook(Box::new(|info| {
        eprintln!("PANIC: {}", info);
        // Also try to write to log file directly
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .append(true)
            .open(std::path::Path::new(&std::env::var("HOME").unwrap_or_default())
                .join(".rustclaw/logs/rustclaw.err"))
        {
            use std::io::Write;
            let _ = writeln!(f, "PANIC at {}: {}", chrono::Local::now(), info);
        }
    }));

    // Initialize logging — file + stderr dual output for daemon mode
    init_logging();

    let cli = Cli::parse();

    match cli.command {
        Commands::Run { config, workspace } => {
            tracing::info!("Starting RustClaw gateway...");
            let cfg = config::load_config(&config)?;
            let workspace_dir = workspace
                .or(cfg.workspace.clone())
                .unwrap_or_else(|| ".".to_string());

            let persona = cfg.persona.as_deref();
            let ws = workspace::Workspace::load_with_persona(&workspace_dir, persona)?;
            tracing::info!("Workspace loaded: {} (persona: {})", workspace_dir, persona.unwrap_or("default"));
            tracing::info!("Agent: {}", ws.identity_name().unwrap_or("unnamed"));

            // Initialize memory (wrap in Arc for tool sharing)
            let persona_name = persona.unwrap_or("default");
            let mut mem = memory::MemoryManager::new(&cfg, &workspace_dir).await?;
            mem = mem.with_namespace(persona_name);
            let mem = std::sync::Arc::new(mem);
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

            // Clean up stale tool result files from previous sessions
            {
                let active = sessions.list_sessions().await;
                let keys: Vec<String> = active.iter().map(|s| s.key.clone()).collect();
                tool_result_storage::cleanup_stale(&keys);
            }

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
                    &cfg,
                )
                .with_spawn_specialist(runner_handle.clone(), Some(orch_ref.clone()))
            } else {
                tools::ToolRegistry::with_defaults_and_memory(&workspace_dir, mem.clone(), &cfg)
                    .with_spawn_specialist(runner_handle.clone(), None)
            };

            // Create LLM client Arc for sharing with ritual tools
            let shared_llm = {
                let client = crate::llm::create_client(&cfg.llm).expect("Failed to create LLM client for ritual");
                std::sync::Arc::new(tokio::sync::RwLock::new(client))
            };
            tools.set_llm_client(shared_llm);

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

            // Clone config receiver for orchestrator hot-reload before passing to runner
            let orch_config_rx = config_rx.clone();

            // Wire config changes to agent runner (hot-reload model, etc.)
            runner.start_config_reload_listener(config_rx);

            // Start heartbeat
            heartbeat::start_heartbeat(
                runner.clone(),
                &cfg.heartbeat,
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

            // Start orchestrator tick loop and config reload listener (if enabled)
            if let Some(ref orch_ref) = orch {
                let tick_interval = cfg.orchestrator.tick_interval;
                // Wire config hot-reload to orchestrator (specialists, tick_interval, max_concurrent)
                let orch_clone = orch_ref.clone();
                orchestrator::start_config_reload_listener(orch_clone.clone(), orch_config_rx).await;
                let runner_clone = runner.clone();
                tokio::spawn(async move {
                    orchestrator::start_orchestrator_loop(orch_clone, runner_clone, tick_interval).await;
                });
            }

            // Start auto-consolidation background task (every 6 hours)
            // Uses spawn_blocking to avoid blocking the tokio runtime with CPU-intensive work.
            let mem_for_reflection = mem_for_consolidation.clone();
            let mem_for_rumination = mem_for_reflection.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(6 * 3600));
                interval.tick().await; // skip first immediate tick
                loop {
                    interval.tick().await;
                    let mem = mem_for_consolidation.clone();
                    match tokio::task::spawn_blocking(move || mem.consolidate()).await {
                        Ok(Ok(())) => tracing::info!("Engram auto-consolidation completed"),
                        Ok(Err(e)) => tracing::warn!("Engram auto-consolidation failed: {}", e),
                        Err(e) => tracing::warn!("Engram auto-consolidation panicked: {}", e),
                    }
                }
            });
            tracing::info!("Engram auto-consolidation scheduled (every 6 hours)");

            // Start rumination background task (every 2 hours)
            // Synthesis only — discovers clusters and generates insights without decaying memory
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(2 * 3600));
                interval.tick().await; // skip first immediate tick
                loop {
                    interval.tick().await;
                    let mem = mem_for_rumination.clone();
                    match tokio::task::spawn_blocking(move || mem.synthesize()).await {
                        Ok(Ok(report)) => {
                            if report.clusters_found > 0 {
                                tracing::info!(
                                    "Synthesis: {} clusters, {} synthesized, {} skipped",
                                    report.clusters_found,
                                    report.clusters_synthesized,
                                    report.clusters_skipped,
                                );
                            }
                        }
                        Ok(Err(e)) => tracing::warn!("Synthesis failed: {}", e),
                        Err(e) => tracing::warn!("Synthesis panicked: {}", e),
                    }
                }
            });
            tracing::info!("Synthesis scheduled (every 2 hours)");

            // Start self-reflection background task (every 24 hours)
            // Decays emotional trends, prunes old logs, logs suggestions
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(24 * 3600));
                interval.tick().await; // skip first immediate tick
                loop {
                    interval.tick().await;
                    let mem = mem_for_reflection.clone();
                    match tokio::task::spawn_blocking(move || mem.self_reflect()).await {
                        Ok(Ok(result)) => {
                            tracing::info!(
                                "Engram self-reflection completed: {} trends decayed, {} logs pruned, {} soul suggestions, {} deprioritized actions",
                                result.trends_decayed,
                                result.logs_pruned,
                                result.soul_suggestions,
                                result.deprioritized_actions
                            );
                        }
                        Ok(Err(e)) => tracing::warn!("Engram self-reflection failed: {}", e),
                        Err(e) => tracing::warn!("Engram self-reflection panicked: {}", e),
                    }
                }
            });
            tracing::info!("Engram self-reflection scheduled (every 24 hours)");

            // Configure token budget alerts
            {
                let tracker = llm::token_tracker();
                tracker.set_hourly_limit(cfg.token_budget.hourly_limit);
                tracing::info!(
                    "Token budget: hourly limit = {}M tokens",
                    cfg.token_budget.hourly_limit / 1_000_000
                );

                // Wire alert to Telegram notification
                if let Some(ref tg_config) = cfg.channels.telegram {
                    let bot_token = tg_config.bot_token.clone();
                    // Send alerts to the first configured chat (potato's DM)
                    tracker.set_alert_fn(move |alert| {
                        let token = bot_token.clone();
                        let msg = alert.message.clone();
                        // Fire-and-forget async send
                        tokio::spawn(async move {
                            let client = reqwest::Client::new();
                            let url = format!("https://api.telegram.org/bot{}/sendMessage", token);
                            let _ = client.post(&url)
                                .json(&serde_json::json!({
                                    "chat_id": 7539582820_i64, // potato's Telegram ID
                                    "text": msg,
                                }))
                                .send()
                                .await;
                        });
                    });
                }
            }

            // Start web dashboard (if enabled)
            dashboard::start_dashboard(cfg.dashboard.clone(), cfg.clone(), runner.clone()).await?;

            // Start channels + graceful shutdown on SIGTERM/SIGINT
            // launchd sends SIGTERM on stop/restart; KeepAlive restarts on exit(0)
            tokio::select! {
                result = channels::start_gateway(cfg, runner) => {
                    result?;
                }
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("Received SIGINT, shutting down gracefully...");
                }
                _ = async {
                    let mut sigterm = tokio::signal::unix::signal(
                        tokio::signal::unix::SignalKind::terminate()
                    ).expect("failed to register SIGTERM handler");
                    sigterm.recv().await;
                } => {
                    tracing::info!("Received SIGTERM, shutting down gracefully...");
                }
            }
            tracing::info!("RustClaw shutdown complete");
        }
        Commands::Chat { config } => {
            interactive_chat(&config).await?;
        }
        Commands::Config { config } => {
            let cfg = config::load_config(&config)?;
            println!("{}", serde_yaml::to_string(&cfg)?);
        }
        Commands::Setup => {
            interactive_setup().await?;
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

/// Interactive chat mode — REPL for talking to the agent.
async fn interactive_chat(config_path: &str) -> anyhow::Result<()> {
    use std::io::{BufRead, Write};

    println!("🐾 RustClaw Interactive Chat");
    println!("Loading config from {}...", config_path);

    let cfg = config::load_config(config_path)?;

    // Setup workspace
    let workspace_dir = cfg.workspace.as_deref().unwrap_or(".");
    let ws = workspace::Workspace::load(workspace_dir)?;

    // Setup memory
    let mem = memory::MemoryManager::new(&cfg, workspace_dir).await?;
    let mem = std::sync::Arc::new(mem);

    // Setup hooks
    let hook_registry = hooks::HookRegistry::new();

    // Setup sessions
    let sessions = session::SessionManager::new(&cfg).await?;

    // Setup tools
    let tools = tools::ToolRegistry::with_defaults_and_memory(workspace_dir, mem.clone(), &cfg);

    // Create agent runner
    let runner = agent::AgentRunner::new(cfg.clone(), ws, mem, sessions, hook_registry, tools);
    let runner = std::sync::Arc::new(runner);

    println!("Model: {}", cfg.llm.model);
    println!("Type 'exit' or 'quit' to leave, '/clear' to clear session.");
    println!();

    let session_key = "interactive:cli";
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    loop {
        print!("You: ");
        stdout.flush()?;

        let mut input = String::new();
        if stdin.lock().read_line(&mut input)? == 0 {
            // EOF
            break;
        }

        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        // Handle special commands
        match input.to_lowercase().as_str() {
            "exit" | "quit" | "/exit" | "/quit" => {
                println!("Goodbye! 👋");
                break;
            }
            "/clear" => {
                runner.clear_session(session_key).await;
                println!("Session cleared.");
                continue;
            }
            "/model" => {
                let model = runner.current_model().await;
                println!("Current model: {}", model);
                continue;
            }
            _ => {}
        }

        // Process with agent
        match runner.process_message(session_key, input, None, Some("cli")).await {
            Ok(response) => {
                let trimmed = response.trim();
                if !trimmed.is_empty() && trimmed != "NO_REPLY" && trimmed != "HEARTBEAT_OK" {
                    println!();
                    println!("Agent: {}", trimmed);
                    println!();
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }

    Ok(())
}

/// Interactive setup wizard — generates rustclaw.yaml.
async fn interactive_setup() -> anyhow::Result<()> {
    use std::io::{BufRead, Write};

    println!("🐾 RustClaw Setup Wizard");
    println!("This will help you create a rustclaw.yaml configuration file.");
    println!();

    fn prompt(question: &str, default: &str) -> String {
        print!("{} [{}]: ", question, default);
        std::io::stdout().flush().ok();
        let mut input = String::new();
        std::io::stdin().lock().read_line(&mut input).ok();
        let input = input.trim();
        if input.is_empty() {
            default.to_string()
        } else {
            input.to_string()
        }
    }

    fn prompt_secret(question: &str) -> String {
        print!("{}: ", question);
        std::io::stdout().flush().ok();
        let mut input = String::new();
        std::io::stdin().lock().read_line(&mut input).ok();
        input.trim().to_string()
    }

    // Workspace
    let workspace = prompt("Workspace directory", ".");

    // LLM Provider
    println!();
    println!("LLM Configuration:");
    let provider = prompt("Provider (anthropic/openai/google)", "anthropic");
    let model = match provider.as_str() {
        "anthropic" => prompt("Model", "claude-sonnet-4-5-20250929"),
        "openai" => prompt("Model", "gpt-4-turbo"),
        "google" => prompt("Model", "gemini-pro"),
        _ => prompt("Model", "claude-sonnet-4-5-20250929"),
    };

    // API Key / Auth Token
    println!();
    println!("Authentication:");
    println!("  For Anthropic, you can use either an API key or OAuth token (Claude Max).");
    let auth_mode = prompt("Auth mode (api_key/oauth)", "oauth");
    let (api_key, auth_token) = if auth_mode == "oauth" {
        let token = prompt_secret("OAuth token (from Claude CLI)");
        (None, Some(token))
    } else {
        let key = prompt_secret("API key");
        (Some(key), None)
    };

    // Telegram
    println!();
    println!("Telegram Configuration (optional):");
    let tg_enabled = prompt("Enable Telegram bot? (y/n)", "y") == "y";
    let (tg_token, tg_users) = if tg_enabled {
        let token = prompt_secret("Telegram bot token (from @BotFather)");
        let users = prompt("Allowed user IDs (comma-separated, empty for all)", "");
        let user_ids: Vec<i64> = users
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        (Some(token), user_ids)
    } else {
        (None, vec![])
    };

    // Generate config
    let mut config_yaml = format!(
        r#"# RustClaw Configuration
# Generated by setup wizard

workspace: "{}"

llm:
  provider: "{}"
  model: "{}"
"#,
        workspace, provider, model
    );

    if let Some(key) = api_key {
        if !key.is_empty() {
            config_yaml.push_str(&format!("  api_key: \"{}\"\n", key));
        }
    }
    if let Some(token) = auth_token {
        if !token.is_empty() {
            config_yaml.push_str(&format!("  auth_token: \"{}\"\n", token));
        }
    }

    config_yaml.push_str("  max_tokens: 8192\n  temperature: 0.7\n");

    if tg_enabled {
        if let Some(token) = tg_token {
            if !token.is_empty() {
                config_yaml.push_str(&format!(
                    r#"
channels:
  telegram:
    bot_token: "{}"
    allowed_users: {:?}
    dm_policy: "owner"
    group_policy: "mention"
"#,
                    token, tg_users
                ));
            }
        }
    }

    config_yaml.push_str(
        r#"
memory:
  auto_recall: true
  auto_store: true
  recall_limit: 5

heartbeat:
  enabled: true
  interval: 3600
  # model: claude-haiku-4-5  # uncomment to use cheaper model for heartbeats
  # quiet_hours: [23, 8]     # uncomment for no heartbeats 23:00-08:00

max_session_messages: 40

dashboard:
  enabled: false
  port: 8080
"#,
    );

    // Write config
    println!();
    let output_path = prompt("Output file", "rustclaw.yaml");
    std::fs::write(&output_path, &config_yaml)?;
    println!("✅ Configuration written to {}", output_path);
    println!();
    println!("Next steps:");
    println!("  1. Review and edit {} as needed", output_path);
    println!("  2. Run: rustclaw daemon start");
    println!("  3. Or:   rustclaw chat");

    Ok(())
}
