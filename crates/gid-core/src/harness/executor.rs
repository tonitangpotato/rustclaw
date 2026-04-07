//! Task executor — trait for spawning sub-agents and CLI-based implementation.
//!
//! The [`TaskExecutor`] trait abstracts sub-agent spawning, allowing different
//! implementations (CLI, API, mock). [`CliExecutor`] spawns the `claude` CLI
//! in a git worktree with a focused prompt (no workspace files).
//!
//! [`ApiExecutor`] uses the agentctl-auth crate's Claude API client directly,
//! providing real token usage statistics and avoiding CLI overhead.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Result;
use async_trait::async_trait;
use tracing::{info, warn, debug};

use super::types::{TaskContext, TaskResult, HarnessConfig};

/// Trait for spawning sub-agents to execute tasks.
///
/// Implementations handle the specifics of how sub-agents are launched
/// (CLI process, API call, in-process mock, etc.).
#[async_trait]
pub trait TaskExecutor: Send + Sync {
    /// Spawn a sub-agent for the given task in the specified worktree.
    ///
    /// Returns a [`TaskResult`] capturing success/failure, output, and usage stats.
    /// Sub-agent failures are data (returned as `TaskResult { success: false, .. }`),
    /// not panics. Only infrastructure errors (process spawn failure, etc.) return `Err`.
    async fn spawn(
        &self,
        context: &TaskContext,
        worktree_path: &Path,
        config: &HarnessConfig,
    ) -> Result<TaskResult>;
}

/// CLI-based executor that spawns `claude` CLI as sub-agents.
///
/// Each task gets a focused system prompt with only the task context
/// (no SOUL.md, AGENTS.md, USER.md, MEMORY.md — GUARD-12).
#[derive(Debug, Clone)]
pub struct CliExecutor {
    /// Path to the claude CLI binary (default: "claude").
    pub claude_bin: String,
}

impl Default for CliExecutor {
    fn default() -> Self {
        Self {
            claude_bin: "claude".to_string(),
        }
    }
}

impl CliExecutor {
    /// Create a new CLI executor with the default claude binary.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a CLI executor with a custom binary path.
    pub fn with_binary(bin: impl Into<String>) -> Self {
        Self {
            claude_bin: bin.into(),
        }
    }

    /// Build the sub-agent prompt from task context.
    ///
    /// The prompt is focused and minimal — no workspace files loaded (GUARD-12).
    /// Contains: task info, goals, design context, guards, verify command.
    pub fn build_prompt(context: &TaskContext) -> String {
        let mut prompt = String::new();

        prompt.push_str("You are a focused coding agent executing a single task.\n\n");

        // Task
        prompt.push_str(&format!("## Your Task\n{}\n\n", context.task_info.title));

        // Description
        if !context.task_info.description.is_empty() {
            prompt.push_str(&format!("## Description\n{}\n\n", context.task_info.description));
        }

        // Goals
        if !context.goals_text.is_empty() {
            prompt.push_str("## Goals\n");
            for goal in &context.goals_text {
                prompt.push_str(&format!("- {}\n", goal));
            }
            prompt.push('\n');
        }

        // Design context
        if let Some(ref excerpt) = context.design_excerpt {
            prompt.push_str(&format!("## Design Context\n{}\n\n", excerpt));
        }

        // Dependency interfaces
        if !context.dependency_interfaces.is_empty() {
            prompt.push_str("## Dependency Interfaces\n");
            for iface in &context.dependency_interfaces {
                prompt.push_str(&format!("- {}\n", iface));
            }
            prompt.push('\n');
        }

        // Guards
        if !context.guards.is_empty() {
            prompt.push_str("## Project Guards (must never be violated)\n");
            for guard in &context.guards {
                prompt.push_str(&format!("- {}\n", guard));
            }
            prompt.push('\n');
        }

        // Verify command
        if let Some(ref verify) = context.task_info.verify {
            prompt.push_str(&format!("## Verify Command\n{}\n\n", verify));
        }

        // Rules
        prompt.push_str("## Rules\n");
        prompt.push_str("1. Stay focused — only implement what's described above\n");
        prompt.push_str("2. Be efficient — write code directly, don't read files unless needed\n");
        prompt.push_str("3. Don't modify .gid/ — graph is managed by the harness\n");
        prompt.push_str("4. Self-test — run the verify command yourself before finishing\n");
        prompt.push_str("5. Report blockers — if you can't complete due to missing dependency, say so clearly\n");

        prompt
    }

    /// Parse usage statistics from claude CLI stderr output.
    ///
    /// Claude CLI with `--verbose` outputs lines like:
    ///   "Total tokens: 12,345"
    ///   "Total turns: 5"
    ///   "Total cost: $0.12"
    /// We also look for non-verbose summary patterns.
    fn parse_usage(stderr: &str) -> (u32, u64) {
        let mut turns: u32 = 0;
        let mut tokens: u64 = 0;

        for line in stderr.lines() {
            let lower = line.to_lowercase();
            // Parse "Total tokens: 12,345" or "tokens: 12345"
            if lower.contains("token") {
                if let Some(num) = Self::extract_number(line) {
                    tokens = num;
                }
            }
            // Parse "Total turns: 5" or "turns: 5"
            if lower.contains("turn") {
                if let Some(num) = Self::extract_number(line) {
                    turns = num as u32;
                }
            }
        }

        (turns, tokens)
    }

    /// Extract the last number from a string (handles commas).
    fn extract_number(s: &str) -> Option<u64> {
        // Find sequences of digits (possibly with commas)
        let cleaned: String = s.chars()
            .rev()
            .take_while(|c| c.is_ascii_digit() || *c == ',')
            .collect::<String>()
            .chars()
            .rev()
            .filter(|c| *c != ',')
            .collect();
        cleaned.parse().ok()
    }

    /// Parse the sub-agent output to detect blockers.
    fn detect_blocker(output: &str) -> Option<String> {
        let lower = output.to_lowercase();
        if lower.contains("blocker:") || lower.contains("blocked by") || lower.contains("cannot proceed") {
            // Extract the blocker line
            for line in output.lines() {
                let ll = line.to_lowercase();
                if ll.contains("blocker:") || ll.contains("blocked by") || ll.contains("cannot proceed") {
                    return Some(line.trim().to_string());
                }
            }
            Some("Sub-agent reported a blocker (details in output)".to_string())
        } else {
            None
        }
    }
}

#[async_trait]
impl TaskExecutor for CliExecutor {
    async fn spawn(
        &self,
        context: &TaskContext,
        worktree_path: &Path,
        config: &HarnessConfig,
    ) -> Result<TaskResult> {
        let prompt = Self::build_prompt(context);
        let start = Instant::now();

        info!(
            task_id = %context.task_info.id,
            worktree = %worktree_path.display(),
            model = %config.model,
            "Spawning sub-agent via CLI"
        );

        // Build command: claude -p "<prompt>" --model <model> --max-turns <n>
        // Note: do NOT use --print — that prevents tool execution (file writes).
        // -p sends the prompt and enables tool use (read/write/exec).
        let output = tokio::process::Command::new(&self.claude_bin)
            .arg("-p")
            .arg(&prompt)
            .arg("--model")
            .arg(&config.model)
            .arg("--max-turns")
            .arg(config.max_iterations.to_string())
            .arg("--allowedTools")
            .arg("Read,Write,Edit,Bash")
            .current_dir(worktree_path)
            .output()
            .await?;

        let _duration = start.elapsed();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let success = output.status.success();
        let combined_output = if stderr.is_empty() {
            stdout.clone()
        } else {
            format!("{}\n--- stderr ---\n{}", stdout, stderr)
        };

        // Auto-commit any changes the sub-agent made in the worktree
        // This is needed for the merge step to have something to merge
        let has_changes = tokio::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(worktree_path)
            .output()
            .await
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(false);

        if has_changes {
            // Stage all changes
            let _ = tokio::process::Command::new("git")
                .args(["add", "-A"])
                .current_dir(worktree_path)
                .output()
                .await;
            // Commit
            let _ = tokio::process::Command::new("git")
                .args(["commit", "-m", &format!("gid: task {} implementation", context.task_info.id)])
                .current_dir(worktree_path)
                .output()
                .await;
        }

        let blocker = Self::detect_blocker(&combined_output);
        let (parsed_turns, parsed_tokens) = Self::parse_usage(&stderr);

        if !success {
            warn!(
                task_id = %context.task_info.id,
                exit_code = ?output.status.code(),
                "Sub-agent exited with non-zero status"
            );
        }

        Ok(TaskResult {
            success,
            output: combined_output,
            turns_used: parsed_turns,
            tokens_used: parsed_tokens,
            blocker,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// API-based Executor (uses agentctl-auth Claude client)
// ═══════════════════════════════════════════════════════════════════════════════

/// API-based executor that uses the Claude Messages API directly.
///
/// Provides real token usage statistics from API responses, unlike the CLI
/// executor which parses stderr output. Supports tool use for Read, Write,
/// Edit, and Bash operations in the task worktree.
#[derive(Debug, Clone)]
pub struct ApiExecutor {
    /// Path to the agentctl auth.toml file.
    pub pool_path: PathBuf,
    /// Timeout for bash commands (default: 30 seconds).
    pub bash_timeout: Duration,
}

impl Default for ApiExecutor {
    fn default() -> Self {
        Self {
            pool_path: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".agentctl")
                .join("auth.toml"),
            bash_timeout: Duration::from_secs(30),
        }
    }
}

impl ApiExecutor {
    /// Create a new API executor with the default auth pool path (~/.agentctl/auth.toml).
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an API executor with a custom pool path.
    pub fn with_pool_path(pool_path: impl Into<PathBuf>) -> Self {
        Self {
            pool_path: pool_path.into(),
            ..Default::default()
        }
    }

    /// Check if the auth pool exists and is usable.
    pub fn is_available(&self) -> bool {
        self.pool_path.exists()
    }

    /// Build tool definitions for the sub-agent.
    fn build_tools() -> Vec<crate::ritual::llm::Tool> {
        use crate::ritual::llm::Tool;
        use serde_json::json;

        vec![
            Tool::new(
                "Read",
                "Read the contents of a file at the specified path. Use this to examine existing code, configuration files, or any text-based files in the project.",
                json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The path to the file to read (relative to the project root)"
                        }
                    },
                    "required": ["path"]
                }),
            ),
            Tool::new(
                "Write",
                "Write content to a file at the specified path. Creates the file if it doesn't exist, or overwrites if it does. Creates parent directories as needed.",
                json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The path to write to (relative to the project root)"
                        },
                        "content": {
                            "type": "string",
                            "description": "The content to write to the file"
                        }
                    },
                    "required": ["path", "content"]
                }),
            ),
            Tool::new(
                "Edit",
                "Make a precise edit to a file by replacing exact text. The old_text must match exactly (including whitespace). Use this for surgical edits to existing files.",
                json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The path to the file to edit (relative to the project root)"
                        },
                        "old_text": {
                            "type": "string",
                            "description": "The exact text to find and replace (must match exactly including whitespace)"
                        },
                        "new_text": {
                            "type": "string",
                            "description": "The new text to replace the old text with"
                        }
                    },
                    "required": ["path", "old_text", "new_text"]
                }),
            ),
            Tool::new(
                "Bash",
                "Execute a shell command. Use this for running tests, build commands, git operations, or any other shell command. Commands run in the project root directory.",
                json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The shell command to execute"
                        }
                    },
                    "required": ["command"]
                }),
            ),
        ]
    }
}

/// Tool handler that executes file/bash operations in a worktree.
struct WorktreeToolHandler {
    worktree_path: PathBuf,
    bash_timeout: Duration,
}

impl WorktreeToolHandler {
    fn new(worktree_path: PathBuf, bash_timeout: Duration) -> Self {
        Self { worktree_path, bash_timeout }
    }

    /// Resolve a path relative to the worktree, ensuring it stays within bounds.
    fn resolve_path(&self, path: &str) -> Result<PathBuf> {
        let resolved = self.worktree_path.join(path);
        let canonical = if resolved.exists() {
            resolved.canonicalize()?
        } else {
            // For non-existent files, canonicalize the parent and append the filename
            if let Some(parent) = resolved.parent() {
                if parent.exists() {
                    let canonical_parent = parent.canonicalize()?;
                    canonical_parent.join(resolved.file_name().unwrap_or_default())
                } else {
                    // Create parent directories for new files
                    std::fs::create_dir_all(parent)?;
                    let canonical_parent = parent.canonicalize()?;
                    canonical_parent.join(resolved.file_name().unwrap_or_default())
                }
            } else {
                resolved
            }
        };

        // Security: ensure the resolved path is within the worktree
        let worktree_canonical = self.worktree_path.canonicalize()?;
        if !canonical.starts_with(&worktree_canonical) {
            anyhow::bail!("Path escapes worktree: {}", path);
        }

        Ok(canonical)
    }

    async fn handle_read(&self, input: &serde_json::Value) -> Result<crate::ritual::llm::ToolOutput> {
        let path = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' field"))?;
        
        let resolved = self.resolve_path(path)?;
        debug!(path = %resolved.display(), "Reading file");
        
        match std::fs::read_to_string(&resolved) {
            Ok(content) => Ok(crate::ritual::llm::ToolOutput::success(content)),
            Err(e) => Ok(crate::ritual::llm::ToolOutput::error(format!("Failed to read {}: {}", path, e))),
        }
    }

    async fn handle_write(&self, input: &serde_json::Value) -> Result<crate::ritual::llm::ToolOutput> {
        let path = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' field"))?;
        let content = input["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'content' field"))?;
        
        let resolved = self.resolve_path(path)?;
        debug!(path = %resolved.display(), bytes = content.len(), "Writing file");
        
        // Create parent directories if needed
        if let Some(parent) = resolved.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        match std::fs::write(&resolved, content) {
            Ok(()) => Ok(crate::ritual::llm::ToolOutput::success(format!("Written {} bytes to {}", content.len(), path))),
            Err(e) => Ok(crate::ritual::llm::ToolOutput::error(format!("Failed to write {}: {}", path, e))),
        }
    }

    async fn handle_edit(&self, input: &serde_json::Value) -> Result<crate::ritual::llm::ToolOutput> {
        let path = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' field"))?;
        let old_text = input["old_text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'old_text' field"))?;
        let new_text = input["new_text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'new_text' field"))?;
        
        let resolved = self.resolve_path(path)?;
        debug!(path = %resolved.display(), "Editing file");
        
        let content = match std::fs::read_to_string(&resolved) {
            Ok(c) => c,
            Err(e) => return Ok(crate::ritual::llm::ToolOutput::error(format!("Failed to read {}: {}", path, e))),
        };
        
        if !content.contains(old_text) {
            return Ok(crate::ritual::llm::ToolOutput::error(format!(
                "old_text not found in {}. Make sure it matches exactly including whitespace.",
                path
            )));
        }
        
        let new_content = content.replacen(old_text, new_text, 1);
        match std::fs::write(&resolved, new_content) {
            Ok(()) => Ok(crate::ritual::llm::ToolOutput::success(format!("Edited {}", path))),
            Err(e) => Ok(crate::ritual::llm::ToolOutput::error(format!("Failed to write {}: {}", path, e))),
        }
    }

    async fn handle_bash(&self, input: &serde_json::Value) -> Result<crate::ritual::llm::ToolOutput> {
        let command = input["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'command' field"))?;
        
        debug!(command = %command, "Executing bash");
        
        let result = tokio::time::timeout(
            self.bash_timeout,
            tokio::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(&self.worktree_path)
                .output(),
        )
        .await;
        
        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let combined = if stderr.is_empty() {
                    stdout.to_string()
                } else {
                    format!("{}\n--- stderr ---\n{}", stdout, stderr)
                };
                
                if output.status.success() {
                    Ok(crate::ritual::llm::ToolOutput::success(combined))
                } else {
                    Ok(crate::ritual::llm::ToolOutput::error(format!(
                        "Command exited with code {}\n{}",
                        output.status.code().unwrap_or(-1),
                        combined
                    )))
                }
            }
            Ok(Err(e)) => Ok(crate::ritual::llm::ToolOutput::error(format!("Failed to execute command: {}", e))),
            Err(_) => Ok(crate::ritual::llm::ToolOutput::error(format!(
                "Command timed out after {} seconds",
                self.bash_timeout.as_secs()
            ))),
        }
    }
}

#[async_trait]
impl crate::ritual::llm::ToolHandler for WorktreeToolHandler {
    async fn handle(&self, name: &str, input: &serde_json::Value) -> Result<crate::ritual::llm::ToolOutput> {
        match name {
            "Read" => self.handle_read(input).await,
            "Write" => self.handle_write(input).await,
            "Edit" => self.handle_edit(input).await,
            "Bash" => self.handle_bash(input).await,
            _ => Ok(crate::ritual::llm::ToolOutput::error(format!("Unknown tool: {}", name))),
        }
    }
}

/// Bridge: wraps a gid-core ToolHandler to implement agentctl_auth::ToolHandler.
struct ApiToolHandlerBridge(WorktreeToolHandler);

#[async_trait]
impl agentctl_auth::ToolHandler for ApiToolHandlerBridge {
    async fn handle(&self, name: &str, input: &serde_json::Value) -> Result<agentctl_auth::ToolOutput> {
        let result = crate::ritual::llm::ToolHandler::handle(&self.0, name, input).await?;
        Ok(agentctl_auth::ToolOutput {
            content: result.content,
            is_error: result.is_error,
        })
    }
}

#[async_trait]
impl TaskExecutor for ApiExecutor {
    async fn spawn(
        &self,
        context: &TaskContext,
        worktree_path: &Path,
        config: &HarnessConfig,
    ) -> Result<TaskResult> {
        let prompt = CliExecutor::build_prompt(context);
        let start = Instant::now();

        info!(
            task_id = %context.task_info.id,
            worktree = %worktree_path.display(),
            model = %config.model,
            "Spawning sub-agent via API"
        );

        // Load auth pool
        let pool = agentctl_auth::AuthPool::load(&self.pool_path)?;
        
        // Build Claude client
        let client = agentctl_auth::claude::Client::builder()
            .pool(&pool)
            .build()?;

        // Build tools (gid-core types) and convert to agentctl-auth types at boundary
        let gid_tools = Self::build_tools();
        let api_tools: Vec<agentctl_auth::Tool> = gid_tools.iter().map(|t| {
            agentctl_auth::Tool::new(&t.name, &t.description, t.input_schema.clone())
        }).collect();
        let handler = ApiToolHandlerBridge(
            WorktreeToolHandler::new(worktree_path.to_path_buf(), self.bash_timeout)
        );

        // System prompt for sub-agent
        let system = "You are a focused coding agent. Complete the task described below. Use the provided tools to read, write, and edit files, and to run commands. Be efficient and precise. When done, provide a brief summary of what you accomplished.";

        // Run agent loop
        let result = client
            .run_agent_loop(
                &config.model,
                system,
                &prompt,
                &api_tools,
                config.max_iterations,
                &handler,
            )
            .await;

        let _duration = start.elapsed();

        match result {
            Ok(loop_result) => {
                // Auto-commit any changes the sub-agent made in the worktree
                let has_changes = tokio::process::Command::new("git")
                    .args(["status", "--porcelain"])
                    .current_dir(worktree_path)
                    .output()
                    .await
                    .map(|o| !o.stdout.is_empty())
                    .unwrap_or(false);

                if has_changes {
                    let _ = tokio::process::Command::new("git")
                        .args(["add", "-A"])
                        .current_dir(worktree_path)
                        .output()
                        .await;
                    let _ = tokio::process::Command::new("git")
                        .args(["commit", "-m", &format!("gid: task {} implementation", context.task_info.id)])
                        .current_dir(worktree_path)
                        .output()
                        .await;
                }

                let blocker = CliExecutor::detect_blocker(&loop_result.final_text);

                info!(
                    task_id = %context.task_info.id,
                    turns = loop_result.turns_used,
                    input_tokens = loop_result.total_input_tokens,
                    output_tokens = loop_result.total_output_tokens,
                    tools_called = loop_result.tool_calls.len(),
                    "Sub-agent completed via API"
                );

                Ok(TaskResult {
                    success: true,
                    output: loop_result.final_text,
                    turns_used: loop_result.turns_used,
                    tokens_used: loop_result.total_input_tokens + loop_result.total_output_tokens,
                    blocker,
                })
            }
            Err(e) => {
                warn!(
                    task_id = %context.task_info.id,
                    error = %e,
                    "Sub-agent failed via API"
                );

                Ok(TaskResult {
                    success: false,
                    output: format!("API error: {}", e),
                    turns_used: 0,
                    tokens_used: 0,
                    blocker: Some(format!("API error: {}", e)),
                })
            }
        }
    }
}

/// Create the appropriate executor based on configuration.
///
/// - `Auto`: If agentctl auth.toml exists, uses ApiExecutor; otherwise CliExecutor.
/// - `Cli`: Always uses CliExecutor.
/// - `Api`: Always uses ApiExecutor (fails if auth pool doesn't exist).
pub fn create_executor(config: &HarnessConfig) -> Box<dyn TaskExecutor> {
    use super::types::ExecutorType;

    match config.executor {
        ExecutorType::Cli => {
            info!("Using CLI executor (configured)");
            Box::new(CliExecutor::new())
        }
        ExecutorType::Api => {
            let api_executor = if let Some(ref path) = config.auth_pool_path {
                ApiExecutor::with_pool_path(path)
            } else {
                ApiExecutor::new()
            };
            info!(pool_path = %api_executor.pool_path.display(), "Using API executor (configured)");
            Box::new(api_executor)
        }
        ExecutorType::Auto => {
            let api_executor = if let Some(ref path) = config.auth_pool_path {
                ApiExecutor::with_pool_path(path)
            } else {
                ApiExecutor::new()
            };
            if api_executor.is_available() {
                info!(pool_path = %api_executor.pool_path.display(), "Using API executor (auto-detected)");
                Box::new(api_executor)
            } else {
                info!("Using CLI executor (no auth pool found)");
                Box::new(CliExecutor::new())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::types::TaskInfo;

    fn sample_context() -> TaskContext {
        TaskContext {
            task_info: TaskInfo {
                id: "auth-impl".to_string(),
                title: "Implement auth module".to_string(),
                description: "Create src/auth.rs with login/logout functions".to_string(),
                goals: vec!["GOAL-1.1".to_string()],
                verify: Some("cargo test --test auth".to_string()),
                estimated_turns: 15,
                depends_on: vec!["config-module".to_string()],
                design_ref: Some("3.2".to_string()),
                satisfies: vec!["GOAL-1.1".to_string()],
            },
            goals_text: vec!["GOAL-1.1: Users can authenticate with API key".to_string()],
            design_excerpt: Some("Section 3.2: Auth module handles token storage".to_string()),
            dependency_interfaces: vec!["config::load() -> Result<Config>".to_string()],
            guards: vec!["GUARD-1: All file writes are atomic".to_string()],
        }
    }

    #[test]
    fn test_build_prompt_includes_all_sections() {
        let ctx = sample_context();
        let prompt = CliExecutor::build_prompt(&ctx);

        assert!(prompt.contains("Implement auth module"), "should contain task title");
        assert!(prompt.contains("src/auth.rs"), "should contain description");
        assert!(prompt.contains("GOAL-1.1"), "should contain goals");
        assert!(prompt.contains("Section 3.2"), "should contain design excerpt");
        assert!(prompt.contains("config::load()"), "should contain dependency interfaces");
        assert!(prompt.contains("GUARD-1"), "should contain guards");
        assert!(prompt.contains("cargo test --test auth"), "should contain verify command");
        assert!(prompt.contains("Stay focused"), "should contain rules");
    }

    #[test]
    fn test_build_prompt_no_workspace_files() {
        let ctx = sample_context();
        let prompt = CliExecutor::build_prompt(&ctx);

        // GUARD-12: No workspace files in sub-agent prompt
        assert!(!prompt.contains("SOUL.md"), "must not reference SOUL.md");
        assert!(!prompt.contains("AGENTS.md"), "must not reference AGENTS.md");
        assert!(!prompt.contains("USER.md"), "must not reference USER.md");
        assert!(!prompt.contains("MEMORY.md"), "must not reference MEMORY.md");
    }

    #[test]
    fn test_detect_blocker() {
        assert!(CliExecutor::detect_blocker("I'm stuck. Blocker: missing config module").is_some());
        assert!(CliExecutor::detect_blocker("Cannot proceed without the auth API").is_some());
        assert!(CliExecutor::detect_blocker("Blocked by missing dependency X").is_some());
        assert!(CliExecutor::detect_blocker("Task completed successfully").is_none());
    }

    #[test]
    fn test_build_prompt_handles_empty_context() {
        let ctx = TaskContext {
            task_info: TaskInfo {
                id: "simple".to_string(),
                title: "Simple task".to_string(),
                description: String::new(),
                goals: vec![],
                verify: None,
                estimated_turns: 10,
                depends_on: vec![],
                design_ref: None,
                satisfies: vec![],
            },
            goals_text: vec![],
            design_excerpt: None,
            dependency_interfaces: vec![],
            guards: vec![],
        };
        let prompt = CliExecutor::build_prompt(&ctx);
        assert!(prompt.contains("Simple task"));
        assert!(prompt.contains("Rules"));
    }
}
