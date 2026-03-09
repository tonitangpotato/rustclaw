//! Configuration loading and types.
//!
//! RustClaw config is a YAML file (rustclaw.yaml) that defines:
//! - LLM provider settings
//! - Channel configs (Telegram, etc.)
//! - Agent definitions (for multi-agent)
//! - Memory settings
//! - Hook configuration

use serde::{Deserialize, Serialize};
use std::path::Path;

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
    // Future: discord, slack, etc.
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
    let config: Config = serde_yaml::from_str(&content)?;
    Ok(config)
}

/// Resolve API key from config or environment variable.
pub fn resolve_api_key(config: &LlmConfig) -> anyhow::Result<String> {
    if let Some(key) = &config.api_key {
        if !key.is_empty() {
            return Ok(key.clone());
        }
    }

    // Try environment variables based on provider
    let env_var = match config.provider.as_str() {
        "anthropic" => "ANTHROPIC_API_KEY",
        "openai" => "OPENAI_API_KEY",
        _ => "LLM_API_KEY",
    };

    std::env::var(env_var).map_err(|_| {
        anyhow::anyhow!(
            "No API key found. Set it in config or via {} env var.",
            env_var
        )
    })
}
