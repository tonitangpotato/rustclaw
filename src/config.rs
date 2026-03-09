//! Configuration loading and types.
//!
//! RustClaw config is a YAML file (rustclaw.yaml) that defines:
//! - LLM provider settings
//! - Channel configs (Telegram, etc.)
//! - Agent definitions (for multi-agent)
//! - Memory settings
//! - Hook configuration
//! - Sandbox settings
//! - Dashboard settings

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::dashboard::DashboardConfig;
use crate::sandbox::SandboxConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Workspace directory (default: current dir)
    pub workspace: Option<String>,

    /// LLM provider configuration
    pub llm: LlmConfig,

    /// Channel configurations
    #[serde(default)]
    pub channels: ChannelsConfig,

    /// Agent definitions (for multi-agent)
    #[serde(default)]
    pub agents: Vec<AgentConfig>,

    /// Memory configuration
    #[serde(default)]
    pub memory: MemoryConfig,

    /// Heartbeat interval in seconds (0 = disabled)
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval: u64,

    /// Maximum messages to keep in session history (default: 40)
    #[serde(default = "default_max_session_messages")]
    pub max_session_messages: usize,

    /// Model to use for session summarization (optional, enables summarization)
    /// Can be a cheaper model like "claude-3-haiku-20240307" for cost savings
    pub summary_model: Option<String>,

    /// Cron jobs
    #[serde(default)]
    pub cron: Vec<CronJobConfig>,

    /// Orchestrator (CEO multi-agent) configuration.
    #[serde(default)]
    pub orchestrator: OrchestratorConfig,

    /// Sandbox configuration for tool execution.
    #[serde(default)]
    pub sandbox: SandboxConfig,

    /// Web dashboard configuration.
    #[serde(default)]
    pub dashboard: DashboardConfig,

    /// Session search configuration.
    #[serde(default)]
    pub search: SearchConfig,

    /// Trajectory export configuration.
    #[serde(default)]
    pub export: ExportConfig,

    /// Browser control configuration.
    #[serde(default)]
    pub browser: BrowserConfig,

    /// Auto skill generation configuration.
    #[serde(default)]
    pub skills: SkillsConfig,

    /// Distributed messaging configuration.
    #[serde(default)]
    pub distributed: DistributedConfig,

    /// Serverless runtime configuration.
    #[serde(default)]
    pub serverless: ServerlessConfig,

    /// GID task graph configuration.
    #[serde(default)]
    pub gid: GidConfig,

    /// Git worktree management configuration.
    #[serde(default)]
    pub worktree: WorktreeConfig,

    /// Credential proxy configuration.
    #[serde(default)]
    pub credential: CredentialConfig,

    /// User modeling configuration.
    #[serde(default)]
    pub user_model: UserModelConfig,

    /// Safety layer configuration.
    #[serde(default)]
    pub safety: crate::safety::SafetyConfig,
}

fn default_max_session_messages() -> usize {
    40
}

fn default_heartbeat_interval() -> u64 {
    1800 // 30 minutes
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Default provider (anthropic, openai, etc.)
    pub provider: String,

    /// Default model
    pub model: String,

    /// API key (or use env var)
    pub api_key: Option<String>,

    /// OAuth token (for Claude Max / claude-cli auth)
    pub auth_token: Option<String>,

    /// API base URL override
    pub base_url: Option<String>,

    /// Max tokens per response
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,

    /// Temperature
    #[serde(default = "default_temperature")]
    pub temperature: f32,
}

fn default_max_tokens() -> u32 {
    8192
}

fn default_temperature() -> f32 {
    0.7
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelsConfig {
    pub telegram: Option<TelegramConfig>,
    pub discord: Option<DiscordConfig>,
    pub slack: Option<SlackConfig>,
    pub signal: Option<SignalConfig>,
    pub whatsapp: Option<crate::channels::whatsapp::WhatsAppConfig>,
    pub matrix: Option<crate::channels::matrix::MatrixConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    /// Bot token from BotFather
    pub bot_token: String,

    /// Allowed user IDs (empty = allow all)
    #[serde(default)]
    pub allowed_users: Vec<i64>,

    /// DM policy: "owner" | "open"
    #[serde(default = "default_dm_policy")]
    pub dm_policy: String,

    /// Group policy: "mention" | "open" | "off"
    #[serde(default = "default_group_policy")]
    pub group_policy: String,
}

fn default_dm_policy() -> String {
    "owner".to_string()
}

fn default_group_policy() -> String {
    "mention".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConfig {
    /// Bot token from Discord Developer Portal
    pub bot_token: String,

    /// Allowed guild (server) IDs (empty = allow all)
    #[serde(default)]
    pub allowed_guilds: Vec<u64>,

    /// Allowed channel IDs (empty = allow all in allowed guilds)
    #[serde(default)]
    pub allowed_channels: Vec<u64>,

    /// Group policy: "mention" | "open" | "off"
    #[serde(default = "default_group_policy")]
    pub group_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackConfig {
    /// Bot OAuth token (xoxb-...)
    pub bot_token: String,

    /// App-level token for Socket Mode (xapp-...)
    pub app_token: String,

    /// Allowed channel IDs (empty = allow all where bot is member)
    #[serde(default)]
    pub allowed_channels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalConfig {
    /// Path to signal-cli executable
    #[serde(default = "default_signal_cli_path")]
    pub signal_cli_path: String,

    /// Phone number registered with Signal (e.g., "+1234567890")
    pub phone_number: String,

    /// Allowed phone numbers that can message (empty = allow all)
    #[serde(default)]
    pub allowed_numbers: Vec<String>,
}

fn default_signal_cli_path() -> String {
    "signal-cli".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Agent ID
    pub id: String,

    /// Display name
    pub name: Option<String>,

    /// Workspace directory (git worktree)
    pub workspace: Option<String>,

    /// Model override
    pub model: Option<String>,

    /// Whether this is the default agent
    #[serde(default)]
    pub default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryConfig {
    /// Engram database path (default: workspace/engram-memory.db)
    pub engram_db: Option<String>,

    /// Enable auto-recall before LLM calls
    #[serde(default = "default_true")]
    pub auto_recall: bool,

    /// Enable auto-store after LLM calls
    #[serde(default = "default_true")]
    pub auto_store: bool,

    /// Max memories to recall per turn
    #[serde(default = "default_recall_limit")]
    pub recall_limit: usize,
}

fn default_true() -> bool {
    true
}

fn default_recall_limit() -> usize {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJobConfig {
    /// Unique job ID.
    pub id: String,

    /// Message to inject into agent.
    pub message: String,

    /// Run every N seconds (mutually exclusive with 'at').
    pub interval_seconds: Option<u64>,

    /// Run once at a specific datetime: "YYYY-MM-DD HH:MM:SS" (mutually exclusive with interval).
    pub at: Option<String>,

    /// Session key for the job (default: cron:{id}).
    pub session_key: Option<String>,

    /// Channel to deliver response (optional).
    pub channel: Option<String>,

    /// Whether the job is enabled (default: true).
    pub enabled: Option<bool>,
}

/// Orchestrator (CEO multi-agent) configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OrchestratorConfig {
    /// Whether the orchestrator is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Tick interval in seconds (how often to check for tasks).
    #[serde(default = "default_tick_interval")]
    pub tick_interval: u64,

    /// Maximum concurrent tasks across all agents.
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: u32,

    /// Specialist agent definitions.
    #[serde(default)]
    pub specialists: Vec<SpecialistConfig>,
}

fn default_tick_interval() -> u64 {
    60
}

fn default_max_concurrent() -> u32 {
    3
}

/// Specialist agent configuration (for orchestrator).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecialistConfig {
    /// Unique agent ID.
    pub id: String,

    /// Display name.
    pub name: Option<String>,

    /// Role for task matching (e.g., "builder", "visibility", "trading").
    pub role: String,

    /// Workspace directory (git worktree path).
    pub workspace: Option<String>,

    /// Model override.
    pub model: Option<String>,

    /// Token budget for this agent (None = unlimited).
    pub budget_tokens: Option<u64>,
}

/// Load config from a YAML file.
pub fn load_config(path: &str) -> anyhow::Result<Config> {
    let path = Path::new(path);
    if !path.exists() {
        anyhow::bail!(
            "Config file not found: {}. Run `rustclaw setup` to create one.",
            path.display()
        );
    }
    let content = std::fs::read_to_string(path)?;
    // Expand environment variables: ${VAR_NAME} → value
    let content = expand_env_vars(&content);
    let config: Config = serde_yaml::from_str(&content)?;
    Ok(config)
}

/// Expand ${VAR_NAME} patterns in a string with environment variable values.
fn expand_env_vars(input: &str) -> String {
    let mut result = input.to_string();
    // Find all ${...} patterns
    while let Some(start) = result.find("${") {
        if let Some(end) = result[start..].find('}') {
            let var_name = &result[start + 2..start + end];
            let value = std::env::var(var_name).unwrap_or_default();
            result = format!("{}{}{}", &result[..start], value, &result[start + end + 1..]);
        } else {
            break;
        }
    }
    result
}

/// Auth mode for the LLM client.
pub enum AuthMode {
    ApiKey(String),
    OAuthToken(String),
}

/// Resolve authentication from config or environment variable.
pub fn resolve_auth(config: &LlmConfig) -> anyhow::Result<AuthMode> {
    // Check OAuth token first
    if let Some(token) = &config.auth_token {
        if !token.is_empty() {
            return Ok(AuthMode::OAuthToken(token.clone()));
        }
    }

    // Check API key
    if let Some(key) = &config.api_key {
        if !key.is_empty() {
            return Ok(AuthMode::ApiKey(key.clone()));
        }
    }

    // Try environment variables
    if let Ok(token) = std::env::var("ANTHROPIC_AUTH_TOKEN") {
        return Ok(AuthMode::OAuthToken(token));
    }
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        return Ok(AuthMode::ApiKey(key));
    }

    let env_var = match config.provider.as_str() {
        "anthropic" => "ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN",
        "openai" => "OPENAI_API_KEY",
        _ => "LLM_API_KEY",
    };

    anyhow::bail!("No auth found. Set api_key/auth_token in config or via {} env var.", env_var)
}

// ─── New Feature Configs ─────────────────────────────────────

/// Session search configuration (FTS5).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    /// Whether session search is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Path to the search database.
    #[serde(default = "default_search_db")]
    pub db_path: String,
}

fn default_search_db() -> String {
    "search.db".to_string()
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            db_path: default_search_db(),
        }
    }
}

/// Trajectory export configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportConfig {
    /// Whether trajectory export is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Output directory for exported trajectories.
    #[serde(default = "default_export_dir")]
    pub output_dir: String,
}

fn default_export_dir() -> String {
    "trajectories".to_string()
}

impl Default for ExportConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            output_dir: default_export_dir(),
        }
    }
}

/// Browser control configuration (CDP).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserConfig {
    /// Whether browser control is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Chrome DevTools debug URL.
    #[serde(default = "default_browser_url")]
    pub debug_url: String,
}

fn default_browser_url() -> String {
    "http://localhost:9222".to_string()
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            debug_url: default_browser_url(),
        }
    }
}

/// Auto skill generation configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsConfig {
    /// Whether skill generation is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Directory to store generated skills.
    #[serde(default = "default_skills_dir")]
    pub skills_dir: String,

    /// Minimum complexity score to trigger skill generation.
    #[serde(default = "default_min_complexity")]
    pub min_complexity: f32,
}

fn default_skills_dir() -> String {
    ".skills".to_string()
}

fn default_min_complexity() -> f32 {
    0.7
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            skills_dir: default_skills_dir(),
            min_complexity: default_min_complexity(),
        }
    }
}

/// Distributed messaging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributedConfig {
    /// Whether distributed messaging is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Unique node ID for this instance.
    #[serde(default = "default_node_id")]
    pub node_id: String,

    /// Address to listen for incoming connections.
    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,

    /// Peer node configurations.
    #[serde(default)]
    pub peers: Vec<PeerConfig>,
}

fn default_node_id() -> String {
    format!("node-{}", uuid::Uuid::new_v4().to_string()[..8].to_string())
}

fn default_listen_addr() -> String {
    "0.0.0.0:9000".to_string()
}

impl Default for DistributedConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            node_id: default_node_id(),
            listen_addr: default_listen_addr(),
            peers: Vec::new(),
        }
    }
}

/// Peer node configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerConfig {
    /// Peer node ID.
    pub node_id: String,
    /// Peer address (host:port).
    pub address: String,
}

/// Serverless runtime configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerlessConfig {
    /// Whether serverless mode is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Idle timeout in seconds before hibernation.
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_secs: u64,

    /// Directory for storing hibernated state.
    #[serde(default = "default_state_dir")]
    pub state_dir: String,
}

fn default_idle_timeout() -> u64 {
    300 // 5 minutes
}

fn default_state_dir() -> String {
    ".rustclaw/hibernated".to_string()
}

impl Default for ServerlessConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            idle_timeout_secs: default_idle_timeout(),
            state_dir: default_state_dir(),
        }
    }
}

/// GID task graph configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GidConfig {
    /// Whether GID is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Path to the graph YAML file.
    #[serde(default = "default_graph_path")]
    pub graph_path: String,
}

fn default_graph_path() -> String {
    ".gid/graph.yml".to_string()
}

impl Default for GidConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            graph_path: default_graph_path(),
        }
    }
}

/// Git worktree management configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeConfig {
    /// Whether worktree management is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Directory for storing worktrees.
    #[serde(default = "default_worktrees_dir")]
    pub worktrees_dir: String,
}

fn default_worktrees_dir() -> String {
    ".rustclaw/worktrees".to_string()
}

impl Default for WorktreeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            worktrees_dir: default_worktrees_dir(),
        }
    }
}

/// Credential proxy configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialConfig {
    /// Whether credential proxy is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Path to the encrypted credentials file.
    #[serde(default = "default_credentials_file")]
    pub credentials_file: String,
}

fn default_credentials_file() -> String {
    ".rustclaw/credentials.enc".to_string()
}

impl Default for CredentialConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            credentials_file: default_credentials_file(),
        }
    }
}

/// User modeling configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserModelConfig {
    /// Whether user modeling is enabled.
    #[serde(default)]
    pub enabled: bool,
}

impl Default for UserModelConfig {
    fn default() -> Self {
        Self { enabled: false }
    }
}
