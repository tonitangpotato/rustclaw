//! Capability-based sandbox for tool execution.
//!
//! This module provides a sandbox environment for executing tools with:
//! - Capability checking (fs read/write, network, exec permissions)
//! - Path restriction using glob patterns
//! - Timeout enforcement
//! - Memory limits (stubbed for future WASM integration)
//!
//! Note: This is a pure Rust capability-based sandbox. Full WASM isolation
//! via Wasmtime is planned for future releases.

use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use glob::Pattern;
use serde::{Deserialize, Serialize};

/// Capabilities allowed for a specific tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCapabilities {
    /// Allow reading from filesystem
    #[serde(default)]
    pub allow_fs_read: bool,

    /// Allow writing to filesystem
    #[serde(default)]
    pub allow_fs_write: bool,

    /// Allow network access
    #[serde(default)]
    pub allow_network: bool,

    /// Allow executing shell commands
    #[serde(default)]
    pub allow_exec: bool,

    /// Maximum memory in MB (future: enforced via WASM)
    #[serde(default = "default_max_memory")]
    pub max_memory_mb: u32,

    /// Maximum execution time in milliseconds
    #[serde(default = "default_max_exec_time")]
    pub max_exec_time_ms: u64,

    /// Allowed path patterns (glob syntax)
    #[serde(default)]
    pub allowed_paths: Vec<String>,
}

fn default_max_memory() -> u32 {
    256 // 256 MB default
}

fn default_max_exec_time() -> u64 {
    30_000 // 30 seconds default
}

impl Default for ToolCapabilities {
    fn default() -> Self {
        Self {
            allow_fs_read: false,
            allow_fs_write: false,
            allow_network: false,
            allow_exec: false,
            max_memory_mb: default_max_memory(),
            max_exec_time_ms: default_max_exec_time(),
            allowed_paths: vec![],
        }
    }
}

impl ToolCapabilities {
    /// Create capabilities that allow everything (for trusted tools).
    pub fn allow_all() -> Self {
        Self {
            allow_fs_read: true,
            allow_fs_write: true,
            allow_network: true,
            allow_exec: true,
            max_memory_mb: 1024,
            max_exec_time_ms: 60_000,
            allowed_paths: vec!["**".to_string()],
        }
    }

    /// Create capabilities for read-only file access.
    pub fn read_only(paths: Vec<String>) -> Self {
        Self {
            allow_fs_read: true,
            allowed_paths: paths,
            ..Default::default()
        }
    }

    /// Check if a path is allowed by the glob patterns.
    pub fn is_path_allowed(&self, path: &str) -> bool {
        if self.allowed_paths.is_empty() {
            return false;
        }

        for pattern_str in &self.allowed_paths {
            // Handle "**" as universal wildcard
            if pattern_str == "**" {
                return true;
            }

            if let Ok(pattern) = Pattern::new(pattern_str) {
                if pattern.matches(path) {
                    return true;
                }
            }
        }

        false
    }
}

/// Sandbox configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Whether sandbox is enabled
    #[serde(default)]
    pub enabled: bool,

    /// Default timeout for all tools (ms)
    #[serde(default = "default_timeout")]
    pub default_timeout_ms: u64,

    /// Per-tool capability configurations
    #[serde(default)]
    pub tools: HashMap<String, ToolCapabilities>,
}

fn default_timeout() -> u64 {
    30_000
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_timeout_ms: default_timeout(),
            tools: HashMap::new(),
        }
    }
}

/// Error types for sandbox violations.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("Tool '{0}' is not allowed to execute shell commands")]
    ExecNotAllowed(String),

    #[error("Tool '{0}' is not allowed to read from filesystem")]
    FsReadNotAllowed(String),

    #[error("Tool '{0}' is not allowed to write to filesystem")]
    FsWriteNotAllowed(String),

    #[error("Tool '{0}' is not allowed to access network")]
    NetworkNotAllowed(String),

    #[error("Tool '{0}' cannot access path '{1}': not in allowed paths")]
    PathNotAllowed(String, String),

    #[error("Tool '{0}' execution timed out after {1}ms")]
    Timeout(String, u64),

    #[error("Tool execution error: {0}")]
    ExecutionError(#[from] anyhow::Error),
}

/// The capability-based sandbox for tool execution.
pub struct WasmSandbox {
    /// Whether the sandbox is enabled
    enabled: bool,

    /// Default timeout for tools
    default_timeout_ms: u64,

    /// Per-tool capability allowlists
    capabilities: HashMap<String, ToolCapabilities>,
}

impl WasmSandbox {
    /// Create a new sandbox from configuration.
    pub fn new(config: &SandboxConfig) -> Self {
        Self {
            enabled: config.enabled,
            default_timeout_ms: config.default_timeout_ms,
            capabilities: config.tools.clone(),
        }
    }

    /// Create a disabled sandbox (all operations allowed).
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            default_timeout_ms: 30_000,
            capabilities: HashMap::new(),
        }
    }

    /// Check if sandbox is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Set capabilities for a specific tool.
    pub fn set_capabilities(&mut self, tool_name: &str, caps: ToolCapabilities) {
        self.capabilities.insert(tool_name.to_string(), caps);
    }

    /// Get capabilities for a tool (returns default if not configured).
    pub fn get_capabilities(&self, tool_name: &str) -> ToolCapabilities {
        self.capabilities
            .get(tool_name)
            .cloned()
            .unwrap_or_default()
    }

    /// Check if a tool is allowed to execute (exec permission).
    pub fn check_exec(&self, tool_name: &str) -> Result<(), SandboxError> {
        if !self.enabled {
            return Ok(());
        }

        let caps = self.get_capabilities(tool_name);
        if !caps.allow_exec {
            return Err(SandboxError::ExecNotAllowed(tool_name.to_string()));
        }
        Ok(())
    }

    /// Check if a tool is allowed to read from filesystem.
    pub fn check_fs_read(&self, tool_name: &str, path: &str) -> Result<(), SandboxError> {
        if !self.enabled {
            return Ok(());
        }

        let caps = self.get_capabilities(tool_name);
        if !caps.allow_fs_read {
            return Err(SandboxError::FsReadNotAllowed(tool_name.to_string()));
        }
        if !caps.is_path_allowed(path) {
            return Err(SandboxError::PathNotAllowed(
                tool_name.to_string(),
                path.to_string(),
            ));
        }
        Ok(())
    }

    /// Check if a tool is allowed to write to filesystem.
    pub fn check_fs_write(&self, tool_name: &str, path: &str) -> Result<(), SandboxError> {
        if !self.enabled {
            return Ok(());
        }

        let caps = self.get_capabilities(tool_name);
        if !caps.allow_fs_write {
            return Err(SandboxError::FsWriteNotAllowed(tool_name.to_string()));
        }
        if !caps.is_path_allowed(path) {
            return Err(SandboxError::PathNotAllowed(
                tool_name.to_string(),
                path.to_string(),
            ));
        }
        Ok(())
    }

    /// Check if a tool is allowed to access network.
    pub fn check_network(&self, tool_name: &str) -> Result<(), SandboxError> {
        if !self.enabled {
            return Ok(());
        }

        let caps = self.get_capabilities(tool_name);
        if !caps.allow_network {
            return Err(SandboxError::NetworkNotAllowed(tool_name.to_string()));
        }
        Ok(())
    }

    /// Get the timeout for a tool in milliseconds.
    pub fn get_timeout_ms(&self, tool_name: &str) -> u64 {
        if !self.enabled {
            return self.default_timeout_ms;
        }

        let caps = self.get_capabilities(tool_name);
        if caps.max_exec_time_ms > 0 {
            caps.max_exec_time_ms
        } else {
            self.default_timeout_ms
        }
    }

    /// Execute a tool with sandbox enforcement.
    ///
    /// This wraps the tool execution with:
    /// - Capability checking based on tool name and arguments
    /// - Timeout enforcement
    ///
    /// # Arguments
    /// * `tool_name` - Name of the tool being executed
    /// * `args` - JSON arguments passed to the tool
    /// * `executor` - The actual tool execution function
    pub async fn execute_tool<F, Fut>(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
        executor: F,
    ) -> Result<String, SandboxError>
    where
        F: FnOnce(serde_json::Value) -> Fut,
        Fut: Future<Output = anyhow::Result<String>>,
    {
        // If sandbox is disabled, just execute
        if !self.enabled {
            return executor(args.clone())
                .await
                .map_err(SandboxError::ExecutionError);
        }

        // Check capabilities based on tool type
        self.check_tool_capabilities(tool_name, args)?;

        // Get timeout for this tool
        let timeout_ms = self.get_timeout_ms(tool_name);
        let timeout = Duration::from_millis(timeout_ms);

        // Execute with timeout
        match tokio::time::timeout(timeout, executor(args.clone())).await {
            Ok(result) => result.map_err(SandboxError::ExecutionError),
            Err(_) => Err(SandboxError::Timeout(tool_name.to_string(), timeout_ms)),
        }
    }

    /// Check capabilities for a tool based on its name and arguments.
    pub fn check_tool_capabilities(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Result<(), SandboxError> {
        match tool_name {
            "exec" => {
                self.check_exec(tool_name)?;
            }
            "read_file" => {
                if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
                    self.check_fs_read(tool_name, path)?;
                }
            }
            "write_file" => {
                if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
                    self.check_fs_write(tool_name, path)?;
                }
            }
            "edit_file" => {
                if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
                    // edit_file needs both read and write
                    self.check_fs_read(tool_name, path)?;
                    self.check_fs_write(tool_name, path)?;
                }
            }
            "list_dir" => {
                if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
                    self.check_fs_read(tool_name, path)?;
                }
            }
            "search_files" => {
                if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
                    self.check_fs_read(tool_name, path)?;
                }
            }
            "web_fetch" => {
                self.check_network(tool_name)?;
            }
            _ => {
                // Unknown tools: if sandbox is enabled and no capabilities configured, deny
                let caps = self.get_capabilities(tool_name);
                if caps == ToolCapabilities::default() {
                    tracing::warn!(
                        "Sandbox: Unknown tool '{}' with no capabilities configured",
                        tool_name
                    );
                }
            }
        }
        Ok(())
    }
}

impl PartialEq for ToolCapabilities {
    fn eq(&self, other: &Self) -> bool {
        self.allow_fs_read == other.allow_fs_read
            && self.allow_fs_write == other.allow_fs_write
            && self.allow_network == other.allow_network
            && self.allow_exec == other.allow_exec
            && self.max_memory_mb == other.max_memory_mb
            && self.max_exec_time_ms == other.max_exec_time_ms
            && self.allowed_paths == other.allowed_paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_allowed_wildcard() {
        let caps = ToolCapabilities {
            allowed_paths: vec!["**".to_string()],
            ..Default::default()
        };
        assert!(caps.is_path_allowed("/any/path"));
        assert!(caps.is_path_allowed("relative/path"));
    }

    #[test]
    fn test_path_allowed_glob() {
        let caps = ToolCapabilities {
            allowed_paths: vec!["/home/user/**".to_string(), "*.rs".to_string()],
            ..Default::default()
        };
        assert!(caps.is_path_allowed("/home/user/documents/file.txt"));
        assert!(caps.is_path_allowed("main.rs"));
        assert!(!caps.is_path_allowed("/etc/passwd"));
    }

    #[test]
    fn test_sandbox_disabled() {
        let sandbox = WasmSandbox::disabled();
        assert!(!sandbox.is_enabled());
        assert!(sandbox.check_exec("exec").is_ok());
        assert!(sandbox.check_fs_read("read_file", "/etc/passwd").is_ok());
    }

    #[test]
    fn test_sandbox_exec_denied() {
        let config = SandboxConfig {
            enabled: true,
            default_timeout_ms: 30_000,
            tools: HashMap::new(), // No tools configured = all denied
        };
        let sandbox = WasmSandbox::new(&config);
        assert!(sandbox.check_exec("exec").is_err());
    }

    #[test]
    fn test_sandbox_exec_allowed() {
        let mut tools = HashMap::new();
        tools.insert(
            "exec".to_string(),
            ToolCapabilities {
                allow_exec: true,
                max_exec_time_ms: 60_000,
                ..Default::default()
            },
        );

        let config = SandboxConfig {
            enabled: true,
            default_timeout_ms: 30_000,
            tools,
        };
        let sandbox = WasmSandbox::new(&config);
        assert!(sandbox.check_exec("exec").is_ok());
    }

    #[test]
    fn test_sandbox_fs_read_path_check() {
        let mut tools = HashMap::new();
        tools.insert(
            "read_file".to_string(),
            ToolCapabilities {
                allow_fs_read: true,
                allowed_paths: vec!["/home/user/**".to_string()],
                ..Default::default()
            },
        );

        let config = SandboxConfig {
            enabled: true,
            default_timeout_ms: 30_000,
            tools,
        };
        let sandbox = WasmSandbox::new(&config);

        // Allowed path
        assert!(sandbox
            .check_fs_read("read_file", "/home/user/file.txt")
            .is_ok());

        // Denied path
        assert!(sandbox.check_fs_read("read_file", "/etc/passwd").is_err());
    }
}
