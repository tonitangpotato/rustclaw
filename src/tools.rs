//! Tool system — exec, file operations, web fetch, memory.
//!
//! Tools are registered in a registry and dispatched by the agent loop.
//! Each tool implements the Tool trait and provides its JSON schema for LLM.

use std::sync::Arc;

use async_trait::async_trait;
use engramai::MemoryType;
use serde::Serialize;
use serde_json::Value;

use gid_core::{
    Graph, Node, Edge, NodeStatus,
    parser::{load_graph as gid_load_graph, save_graph as gid_save_graph},
    query::QueryEngine,
    validator::Validator,
    visual::{render, VisualFormat},
    advise::{analyze as advise_analyze},
    history::HistoryManager,
    refactor,
};
use crate::config::AgentConfig;
use crate::memory::MemoryManager;
use crate::orchestrator::{SharedOrchestrator, Task};

/// Shared handle to AgentRunner for late-binding (used by SpawnSpecialistTool).
/// Initially None, set after AgentRunner is created.
pub type SharedAgentRunner = Arc<tokio::sync::RwLock<Option<Arc<crate::agent::AgentRunner>>>>;

/// Result of a tool execution.
#[derive(Debug, Clone, Serialize)]
pub struct ToolResult {
    pub output: String,
    pub is_error: bool,
}

/// Trait for implementing tools.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Tool name (used by LLM to call it).
    fn name(&self) -> &str;

    /// Human-readable description.
    fn description(&self) -> &str;

    /// JSON Schema for input parameters.
    fn input_schema(&self) -> Value;

    /// Execute the tool with given input.
    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult>;
}

/// Registry that manages all available tools.
pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// Register all default tools.
    pub fn with_defaults(workspace_root: &str) -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(ExecTool));
        registry.register(Box::new(ReadFileTool::new(workspace_root)));
        registry.register(Box::new(WriteFileTool::new(workspace_root)));
        registry.register(Box::new(ListDirTool::new(workspace_root)));
        registry.register(Box::new(WebFetchTool));
        registry.register(Box::new(EditFileTool::new(workspace_root)));
        registry.register(Box::new(SearchFilesTool::new(workspace_root)));
        registry
    }

    /// Register core tools for sub-agents (no engram, no GID, no orchestrator tools).
    /// Tools are scoped to the given workspace root.
    pub fn for_subagent(workspace_root: &str) -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(ExecTool));
        registry.register(Box::new(ReadFileTool::new(workspace_root)));
        registry.register(Box::new(WriteFileTool::new(workspace_root)));
        registry.register(Box::new(ListDirTool::new(workspace_root)));
        registry.register(Box::new(EditFileTool::new(workspace_root)));
        registry.register(Box::new(SearchFilesTool::new(workspace_root)));
        registry.register(Box::new(WebFetchTool));
        registry
    }
    
    /// Register all default tools including memory tools.
    pub fn with_defaults_and_memory(workspace_root: &str, memory: Arc<MemoryManager>) -> Self {
        let mut registry = Self::with_defaults(workspace_root);
        registry.register(Box::new(EngramRecallTool::new(memory.clone())));
        registry.register(Box::new(EngramStoreTool::new(memory.clone())));
        registry.register(Box::new(EngramRecallAssociatedTool::new(memory)));
        registry
    }

    /// Register all default tools including memory and orchestrator tools.
    pub fn with_defaults_and_orchestrator(
        workspace_root: &str,
        memory: Arc<MemoryManager>,
        orchestrator: SharedOrchestrator,
    ) -> Self {
        let mut registry = Self::with_defaults_and_memory(workspace_root, memory);
        registry.register(Box::new(DelegateTaskTool::new(orchestrator)));
        registry
    }

    /// Register the spawn_specialist tool with access to the agent runner.
    /// The runner handle is initially empty and must be set after AgentRunner creation.
    pub fn with_spawn_specialist(mut self, runner: SharedAgentRunner, orchestrator: Option<SharedOrchestrator>) -> Self {
        self.register(Box::new(SpawnSpecialistTool::new(runner, orchestrator)));
        self
    }

    /// Register GID (task graph) tools.
    pub fn with_gid(mut self, graph_path: &str) -> Self {
        let graph = Arc::new(tokio::sync::RwLock::new(
            gid_load_graph(std::path::Path::new(graph_path)).unwrap_or_default()
        ));
        let path = Arc::new(graph_path.to_string());

        // Original 5 tools (backward compatible)
        self.register(Box::new(GidTasksTool::new(graph.clone())));
        self.register(Box::new(GidAddTaskTool::new(graph.clone(), path.clone())));
        self.register(Box::new(GidUpdateTaskTool::new(graph.clone(), path.clone())));
        self.register(Box::new(GidAddEdgeTool::new(graph.clone(), path.clone())));
        self.register(Box::new(GidReadTool::new(graph.clone())));

        // New gid-core tools
        self.register(Box::new(GidCompleteTool::new(graph.clone(), path.clone())));
        self.register(Box::new(GidQueryImpactTool::new(graph.clone())));
        self.register(Box::new(GidQueryDepsTool::new(graph.clone())));
        self.register(Box::new(GidValidateTool::new(graph.clone())));
        self.register(Box::new(GidAdviseTool::new(graph.clone())));
        self.register(Box::new(GidVisualTool::new(graph.clone())));
        self.register(Box::new(GidHistoryTool::new(path.clone())));
        self.register(Box::new(GidRefactorTool::new(graph.clone(), path.clone())));

        self
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        tracing::debug!("Registered tool: {}", tool.name());
        self.tools.push(tool);
    }

    /// Get LLM tool definitions for all registered tools.
    pub fn definitions(&self) -> Vec<crate::llm::ToolDefinition> {
        self.tools
            .iter()
            .map(|t| crate::llm::ToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                input_schema: t.input_schema(),
            })
            .collect()
    }

    /// Execute a tool by name.
    pub async fn execute(&self, name: &str, input: Value) -> anyhow::Result<ToolResult> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.name() == name)
            .ok_or_else(|| anyhow::anyhow!("Unknown tool: {}", name))?;

        tool.execute(input).await
    }
}

// ─── Exec Tool ───────────────────────────────────────────────

/// Execute shell commands.
pub struct ExecTool;

#[async_trait]
impl Tool for ExecTool {
    fn name(&self) -> &str {
        "exec"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return its output."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to execute"
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory (optional)"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Timeout in seconds (default: 30)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let command = input["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'command' parameter"))?;

        let workdir = input["workdir"].as_str();
        let timeout_secs = input["timeout_secs"].as_u64().unwrap_or(30);

        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c").arg(command);

        if let Some(dir) = workdir {
            cmd.current_dir(dir);
        }

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            cmd.output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Command timed out after {}s", timeout_secs))?
        .map_err(|e| anyhow::anyhow!("Failed to execute command: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut result = String::new();
        if !stdout.is_empty() {
            result.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str("STDERR: ");
            result.push_str(&stderr);
        }

        // Truncate if too long
        if result.len() > 50_000 {
            result.truncate(50_000);
            result.push_str("\n... (truncated)");
        }

        Ok(ToolResult {
            output: if result.is_empty() {
                format!("(exit code: {})", output.status.code().unwrap_or(-1))
            } else {
                result
            },
            is_error: !output.status.success(),
        })
    }
}

// ─── Read File Tool ──────────────────────────────────────────

pub struct ReadFileTool {
    workspace_root: String,
}

impl ReadFileTool {
    pub fn new(workspace_root: &str) -> Self {
        Self {
            workspace_root: workspace_root.to_string(),
        }
    }
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file (relative to workspace or absolute)"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line number to start reading from (1-indexed)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;

        let path = if std::path::Path::new(path_str).is_absolute() {
            std::path::PathBuf::from(path_str)
        } else {
            std::path::PathBuf::from(&self.workspace_root).join(path_str)
        };

        if !path.exists() {
            return Ok(ToolResult {
                output: format!("File not found: {}", path.display()),
                is_error: true,
            });
        }

        let content = tokio::fs::read_to_string(&path).await?;
        let lines: Vec<&str> = content.lines().collect();

        let offset = input["offset"].as_u64().unwrap_or(1).max(1) as usize - 1;
        let limit = input["limit"].as_u64().unwrap_or(2000) as usize;

        let selected: Vec<&str> = lines.iter().skip(offset).take(limit).copied().collect();
        let result = selected.join("\n");

        // Truncate if too long
        let result = if result.len() > 50_000 {
            format!(
                "{}\n... (truncated, {} total lines)",
                crate::text_utils::truncate_bytes(&result, 50_000),
                lines.len()
            )
        } else if lines.len() > offset + limit {
            format!(
                "{}\n\n[{} more lines. Use offset={} to continue.]",
                result,
                lines.len() - offset - limit,
                offset + limit + 1
            )
        } else {
            result
        };

        Ok(ToolResult {
            output: result,
            is_error: false,
        })
    }
}

// ─── Write File Tool ─────────────────────────────────────────

pub struct WriteFileTool {
    workspace_root: String,
}

impl WriteFileTool {
    pub fn new(workspace_root: &str) -> Self {
        Self {
            workspace_root: workspace_root.to_string(),
        }
    }
}

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates parent directories if needed."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;
        let content = input["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'content' parameter"))?;

        let path = if std::path::Path::new(path_str).is_absolute() {
            std::path::PathBuf::from(path_str)
        } else {
            std::path::PathBuf::from(&self.workspace_root).join(path_str)
        };

        // Create parent directories
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(&path, content).await?;

        Ok(ToolResult {
            output: format!("Wrote {} bytes to {}", content.len(), path.display()),
            is_error: false,
        })
    }
}

// ─── List Directory Tool ─────────────────────────────────────

pub struct ListDirTool {
    workspace_root: String,
}

impl ListDirTool {
    pub fn new(workspace_root: &str) -> Self {
        Self {
            workspace_root: workspace_root.to_string(),
        }
    }
}

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List files and directories in a path."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path to list (default: workspace root)"
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let path_str = input["path"].as_str().unwrap_or(".");

        let path = if std::path::Path::new(path_str).is_absolute() {
            std::path::PathBuf::from(path_str)
        } else {
            std::path::PathBuf::from(&self.workspace_root).join(path_str)
        };

        if !path.exists() {
            return Ok(ToolResult {
                output: format!("Directory not found: {}", path.display()),
                is_error: true,
            });
        }

        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(&path).await?;
        while let Some(entry) = dir.next_entry().await? {
            let file_type = entry.file_type().await?;
            let name = entry.file_name().to_string_lossy().to_string();
            let suffix = if file_type.is_dir() { "/" } else { "" };
            entries.push(format!("{}{}", name, suffix));
        }

        entries.sort();

        Ok(ToolResult {
            output: entries.join("\n"),
            is_error: false,
        })
    }
}

// ─── Web Fetch Tool ──────────────────────────────────────────

/// Fetch and extract readable content from a URL.
pub struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch and extract readable content from a URL (HTML → text)."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL to fetch"
                },
                "max_chars": {
                    "type": "integer",
                    "description": "Maximum characters to return (default: 50000)"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let url = input["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'url' parameter"))?;
        let max_chars = input["max_chars"].as_u64().unwrap_or(50_000) as usize;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("Mozilla/5.0 (compatible; RustClaw/0.1)")
            .build()?;

        let resp = client.get(url).send().await?;
        let status = resp.status();

        if !status.is_success() {
            return Ok(ToolResult {
                output: format!("HTTP error: {}", status),
                is_error: true,
            });
        }

        let body = resp.text().await?;

        // Basic HTML tag stripping for MVP
        let text = strip_html_basic(&body);

        let result = if text.len() > max_chars {
            format!("{}\n... (truncated)", crate::text_utils::truncate_bytes(&text, max_chars))
        } else {
            text
        };

        Ok(ToolResult {
            output: result,
            is_error: false,
        })
    }
}

/// Basic HTML tag stripping.
fn strip_html_basic(html: &str) -> String {
    let mut result = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let mut last_was_space = false;

    for c in html.chars() {
        if c == '<' {
            in_tag = true;
            continue;
        }
        if c == '>' {
            in_tag = false;
            continue;
        }
        if in_tag {
            continue;
        }
        if c.is_whitespace() {
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else {
            result.push(c);
            last_was_space = false;
        }
    }

    result.trim().to_string()
}

// ─── Edit File Tool ──────────────────────────────────────────

/// Surgical file editing — find and replace exact text.
pub struct EditFileTool {
    workspace_root: String,
}

impl EditFileTool {
    pub fn new(workspace_root: &str) -> Self {
        Self {
            workspace_root: workspace_root.to_string(),
        }
    }
}

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Edit a file by replacing exact text. The old_string must match exactly (including whitespace)."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "old_string": {
                    "type": "string",
                    "description": "Exact text to find and replace"
                },
                "new_string": {
                    "type": "string",
                    "description": "New text to replace with"
                }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path'"))?;
        let old_string = input["old_string"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'old_string'"))?;
        let new_string = input["new_string"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'new_string'"))?;

        let path = if std::path::Path::new(path_str).is_absolute() {
            std::path::PathBuf::from(path_str)
        } else {
            std::path::PathBuf::from(&self.workspace_root).join(path_str)
        };

        if !path.exists() {
            return Ok(ToolResult {
                output: format!("File not found: {}", path.display()),
                is_error: true,
            });
        }

        let content = tokio::fs::read_to_string(&path).await?;

        let count = content.matches(old_string).count();
        if count == 0 {
            return Ok(ToolResult {
                output: "old_string not found in file. Make sure it matches exactly (including whitespace).".to_string(),
                is_error: true,
            });
        }
        if count > 1 {
            return Ok(ToolResult {
                output: format!("old_string found {} times. It must be unique. Add more context to disambiguate.", count),
                is_error: true,
            });
        }

        let new_content = content.replacen(old_string, new_string, 1);
        tokio::fs::write(&path, &new_content).await?;

        Ok(ToolResult {
            output: format!("Edited {}", path.display()),
            is_error: false,
        })
    }
}

// ─── Search Files Tool ───────────────────────────────────────

/// Search for text patterns across files (like grep).
pub struct SearchFilesTool {
    workspace_root: String,
}

impl SearchFilesTool {
    pub fn new(workspace_root: &str) -> Self {
        Self {
            workspace_root: workspace_root.to_string(),
        }
    }
}

#[async_trait]
impl Tool for SearchFilesTool {
    fn name(&self) -> &str {
        "search_files"
    }

    fn description(&self) -> &str {
        "Search for a text pattern in files (recursive grep). Returns matching lines with file paths."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Text pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in (default: workspace root)"
                },
                "file_pattern": {
                    "type": "string",
                    "description": "File name pattern to match (e.g. '*.rs', '*.md')"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let pattern = input["pattern"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'pattern'"))?;
        let search_path = input["path"].as_str().unwrap_or(".");
        let file_pattern = input["file_pattern"].as_str();

        let path = if std::path::Path::new(search_path).is_absolute() {
            std::path::PathBuf::from(search_path)
        } else {
            std::path::PathBuf::from(&self.workspace_root).join(search_path)
        };

        // Use grep for efficiency
        let mut cmd_str = format!(
            "grep -rn --include='*.rs' --include='*.md' --include='*.toml' --include='*.yaml' --include='*.yml' --include='*.json' --include='*.txt' --include='*.py' --include='*.ts' --include='*.js' {} {}",
            shell_escape(pattern),
            shell_escape(path.to_str().unwrap_or("."))
        );

        if let Some(fp) = file_pattern {
            cmd_str = format!(
                "grep -rn --include='{}' {} {}",
                fp,
                shell_escape(pattern),
                shell_escape(path.to_str().unwrap_or("."))
            );
        }

        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd_str)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        if stdout.is_empty() {
            return Ok(ToolResult {
                output: format!("No matches found for '{}'", pattern),
                is_error: false,
            });
        }

        // Limit output
        let result = if stdout.len() > 30_000 {
            format!("{}\n... (truncated, too many matches)", crate::text_utils::truncate_bytes(&stdout, 30_000))
        } else {
            stdout.to_string()
        };

        Ok(ToolResult {
            output: result,
            is_error: false,
        })
    }
}

/// Simple shell escaping for command arguments.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

// ─── Engram Recall Tool ──────────────────────────────────────

/// Search memories using Engram.
pub struct EngramRecallTool {
    memory: Arc<MemoryManager>,
}

impl EngramRecallTool {
    pub fn new(memory: Arc<MemoryManager>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for EngramRecallTool {
    fn name(&self) -> &str {
        "engram_recall"
    }

    fn description(&self) -> &str {
        "Search your memories for relevant information. Use this to recall past conversations, facts, preferences, or any previously stored knowledge."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query to find relevant memories"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of memories to return (default: 5)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let query = input["query"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' parameter"))?;
        let limit = input["limit"].as_u64().unwrap_or(5) as usize;

        match self.memory.recall_explicit(query, limit) {
            Ok(memories) => {
                if memories.is_empty() {
                    return Ok(ToolResult {
                        output: "No relevant memories found.".to_string(),
                        is_error: false,
                    });
                }

                let mut output = format!("Found {} relevant memories:\n\n", memories.len());
                for (i, mem) in memories.iter().enumerate() {
                    output.push_str(&format!(
                        "{}. [{}] (confidence: {:.2})\n   {}\n\n",
                        i + 1,
                        mem.memory_type,
                        mem.confidence,
                        mem.content
                    ));
                }

                Ok(ToolResult {
                    output,
                    is_error: false,
                })
            }
            Err(e) => Ok(ToolResult {
                output: format!("Memory recall failed: {}", e),
                is_error: true,
            }),
        }
    }
}

// ─── Engram Store Tool ───────────────────────────────────────

/// Store new memories using Engram.
pub struct EngramStoreTool {
    memory: Arc<MemoryManager>,
}

impl EngramStoreTool {
    pub fn new(memory: Arc<MemoryManager>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for EngramStoreTool {
    fn name(&self) -> &str {
        "engram_store"
    }

    fn description(&self) -> &str {
        "Store important information in memory for future recall. Use this to remember facts, preferences, lessons learned, or important events."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "The information to remember"
                },
                "memory_type": {
                    "type": "string",
                    "enum": ["factual", "episodic", "procedural", "relational", "emotional", "opinion", "causal"],
                    "description": "Type of memory: factual (facts), episodic (events), procedural (how-to), relational (people/connections), emotional (feelings), opinion (preferences), causal (cause/effect)"
                },
                "importance": {
                    "type": "number",
                    "description": "Importance score from 0.0 to 1.0 (default: 0.5)"
                }
            },
            "required": ["content", "memory_type"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let content = input["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'content' parameter"))?;
        let memory_type_str = input["memory_type"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'memory_type' parameter"))?;
        let importance = input["importance"].as_f64().unwrap_or(0.5);

        let memory_type = match memory_type_str.to_lowercase().as_str() {
            "factual" => MemoryType::Factual,
            "episodic" => MemoryType::Episodic,
            "procedural" => MemoryType::Procedural,
            "relational" => MemoryType::Relational,
            "emotional" => MemoryType::Emotional,
            "opinion" => MemoryType::Opinion,
            "causal" => MemoryType::Causal,
            _ => {
                return Ok(ToolResult {
                    output: format!(
                        "Invalid memory_type '{}'. Must be one of: factual, episodic, procedural, relational, emotional, opinion, causal",
                        memory_type_str
                    ),
                    is_error: true,
                });
            }
        };

        match self.memory.store_explicit(content, memory_type, importance) {
            Ok(()) => Ok(ToolResult {
                output: format!(
                    "Memory stored successfully: {} (type: {}, importance: {:.2})",
                    if content.chars().count() > 50 {
                        format!("{}...", content.chars().take(50).collect::<String>())
                    } else {
                        content.to_string()
                    },
                    memory_type_str,
                    importance
                ),
                is_error: false,
            }),
            Err(e) => Ok(ToolResult {
                output: format!("Failed to store memory: {}", e),
                is_error: true,
            }),
        }
    }
}

// ─── Engram Recall Associated Tool ───────────────────────────

/// Recall associated memories using Hebbian links.
pub struct EngramRecallAssociatedTool {
    memory: Arc<MemoryManager>,
}

impl EngramRecallAssociatedTool {
    pub fn new(memory: Arc<MemoryManager>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for EngramRecallAssociatedTool {
    fn name(&self) -> &str {
        "engram_recall_associated"
    }

    fn description(&self) -> &str {
        "Recall associated/causal memories — memories about cause→effect relationships or things that frequently co-occur. Use this to find related patterns, consequences, or correlated events."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Optional search query to find relevant associated memories. If omitted, returns all causal memories sorted by importance."
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of memories to return (default: 5)"
                },
                "min_confidence": {
                    "type": "number",
                    "description": "Minimum confidence threshold 0.0-1.0 (default: 0.0)"
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let query = input["query"].as_str();
        let limit = input["limit"].as_u64().unwrap_or(5) as usize;
        let min_confidence = input["min_confidence"].as_f64().unwrap_or(0.0);

        match self.memory.recall_associated(query, limit, min_confidence) {
            Ok(memories) => {
                if memories.is_empty() {
                    return Ok(ToolResult {
                        output: "No associated memories found.".to_string(),
                        is_error: false,
                    });
                }

                let mut output = format!("Found {} associated memories:\n\n", memories.len());
                for (i, mem) in memories.iter().enumerate() {
                    let label = mem.confidence_label.as_deref().unwrap_or("likely");
                    output.push_str(&format!(
                        "{}. [{}] (confidence: {:.2})\n   {}\n\n",
                        i + 1,
                        label,
                        mem.confidence,
                        mem.content
                    ));
                }

                Ok(ToolResult {
                    output,
                    is_error: false,
                })
            }
            Err(e) => Ok(ToolResult {
                output: format!("Failed to recall associated memories: {}", e),
                is_error: true,
            }),
        }
    }
}

// ─── Delegate Task Tool ──────────────────────────────────────

/// Delegate a task to a specialist agent via the orchestrator.
/// Waits for task completion (with timeout) before returning the result.
pub struct DelegateTaskTool {
    orchestrator: SharedOrchestrator,
}

impl DelegateTaskTool {
    pub fn new(orchestrator: SharedOrchestrator) -> Self {
        Self { orchestrator }
    }
}

#[async_trait]
impl Tool for DelegateTaskTool {
    fn name(&self) -> &str {
        "delegate_task"
    }

    fn description(&self) -> &str {
        "Delegate a task to a specialist agent and wait for completion. The task will be assigned to an appropriate agent based on role matching. Returns the result when the specialist agent completes the task."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "description": {
                    "type": "string",
                    "description": "Detailed description of the task for the specialist agent"
                },
                "roles": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Roles that can handle this task (e.g., ['builder'], ['visibility']). Leave empty to allow any specialist."
                },
                "priority": {
                    "type": "integer",
                    "description": "Priority (0=highest, 255=lowest). Default: 100"
                },
                "budget_tokens": {
                    "type": "integer",
                    "description": "Maximum tokens this task can use (optional)"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Maximum seconds to wait for completion (default: 600 = 10 minutes)"
                },
                "wait": {
                    "type": "boolean",
                    "description": "Whether to wait for completion (default: true). Set to false for fire-and-forget."
                }
            },
            "required": ["description"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let description = input["description"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'description' parameter"))?;

        let roles: Vec<String> = input["roles"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let priority = input["priority"].as_u64().unwrap_or(100) as u8;
        let budget_tokens = input["budget_tokens"].as_u64();
        let timeout_secs = input["timeout_secs"].as_u64().unwrap_or(600);
        let wait = input["wait"].as_bool().unwrap_or(true);

        // Generate task ID
        let task_id = format!("task_{}", uuid::Uuid::new_v4().to_string()[..8].to_string());

        // Create the task
        let mut task = Task::new(&task_id, description)
            .with_priority(priority)
            .with_roles(roles.clone());

        if let Some(budget) = budget_tokens {
            task = task.with_budget(budget);
        }

        // Submit to orchestrator
        {
            let mut orch = self.orchestrator.write().await;
            orch.submit_task(task);
        }

        if !wait {
            // Fire-and-forget mode
            return Ok(ToolResult {
                output: format!(
                    "Task '{}' submitted to orchestrator (fire-and-forget).\n- Description: {}\n- Roles: {:?}\n- Priority: {}",
                    task_id,
                    if description.chars().count() > 100 {
                        format!("{}...", description.chars().take(100).collect::<String>())
                    } else {
                        description.to_string()
                    },
                    if roles.is_empty() { vec!["any".to_string()] } else { roles },
                    priority
                ),
                is_error: false,
            });
        }

        // Wait for task completion (polling)
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(timeout_secs);
        let poll_interval = std::time::Duration::from_millis(500);

        loop {
            // Check if timeout exceeded
            if start.elapsed() > timeout {
                return Ok(ToolResult {
                    output: format!(
                        "Task '{}' timed out after {} seconds. The task may still be running.",
                        task_id, timeout_secs
                    ),
                    is_error: true,
                });
            }

            // Check task status
            let task_status = {
                let orch = self.orchestrator.read().await;
                orch.get_task(&task_id).map(|t| (t.status.clone(), t.result.clone(), t.error.clone()))
            };

            match task_status {
                Some((crate::orchestrator::TaskStatus::Done, result, _)) => {
                    let output = result.unwrap_or_else(|| "(no result)".to_string());
                    return Ok(ToolResult {
                        output: format!(
                            "Task '{}' completed successfully.\n\n## Result:\n{}",
                            task_id, output
                        ),
                        is_error: false,
                    });
                }
                Some((crate::orchestrator::TaskStatus::Failed, _, error)) => {
                    let err_msg = error.unwrap_or_else(|| "(unknown error)".to_string());
                    return Ok(ToolResult {
                        output: format!("Task '{}' failed: {}", task_id, err_msg),
                        is_error: true,
                    });
                }
                Some((crate::orchestrator::TaskStatus::Cancelled, _, _)) => {
                    return Ok(ToolResult {
                        output: format!("Task '{}' was cancelled.", task_id),
                        is_error: true,
                    });
                }
                Some((crate::orchestrator::TaskStatus::Pending, _, _)) |
                Some((crate::orchestrator::TaskStatus::InProgress, _, _)) => {
                    // Still running, continue polling
                }
                None => {
                    return Ok(ToolResult {
                        output: format!("Task '{}' not found in orchestrator.", task_id),
                        is_error: true,
                    });
                }
            }

            // Wait before next poll
            tokio::time::sleep(poll_interval).await;
        }
    }
}

// ─── Spawn Specialist Tool ───────────────────────────────────

/// Spawn a sub-agent on-demand for a specific task.
/// Works with or without orchestrator — uses orchestrator specialists if available,
/// otherwise spawns an ad-hoc sub-agent directly.
pub struct SpawnSpecialistTool {
    runner: SharedAgentRunner,
    orchestrator: Option<SharedOrchestrator>,
}

impl SpawnSpecialistTool {
    pub fn new(runner: SharedAgentRunner, orchestrator: Option<SharedOrchestrator>) -> Self {
        Self { runner, orchestrator }
    }


}

#[async_trait]
impl Tool for SpawnSpecialistTool {
    fn name(&self) -> &str {
        "spawn_specialist"
    }

    fn description(&self) -> &str {
        "Spawn a sub-agent to handle a specific task. The sub-agent runs with its own agentic loop and tool access. Use this for delegating complex work that requires multiple tool calls."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "Task description/prompt for the sub-agent"
                },
                "role": {
                    "type": "string",
                    "description": "Role to match (e.g., 'builder', 'research'). If matching specialist exists, uses its config."
                },
                "model": {
                    "type": "string",
                    "description": "Model override (e.g., 'claude-sonnet-4-5'). Default: use parent's model."
                },
                "workspace": {
                    "type": "string",
                    "description": "Working directory for the sub-agent. Default: parent's workspace."
                },
                "max_iterations": {
                    "type": "integer",
                    "description": "Maximum tool loop iterations (default: 25)"
                },
                "wait": {
                    "type": "boolean",
                    "description": "Whether to wait for completion (default: true). If false, returns immediately with task ID."
                }
            },
            "required": ["task"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        // Get runner from shared handle
        let runner_guard = self.runner.read().await;
        let runner = runner_guard.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Agent runner not initialized"))?;

        // Parse input parameters
        let task = input["task"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'task' parameter"))?;

        let role = input["role"].as_str();
        let model_override = input["model"].as_str();
        let workspace_override = input["workspace"].as_str();
        let max_iterations = input["max_iterations"].as_u64().unwrap_or(25) as u32;
        let wait = input["wait"].as_bool().unwrap_or(true);

        // Generate a unique task/session ID
        let task_id = format!("spawn_{}", uuid::Uuid::new_v4().to_string()[..8].to_string());

        // Try to find matching specialist config from orchestrator or config
        let agent_config = if let Some(role_name) = role {
            // Check if orchestrator has a specialist with this ID
            let from_orchestrator = if let Some(ref orch) = self.orchestrator {
                let orch = orch.read().await;
                orch.get_agent(role_name).map(|a| a.to_agent_config())
            } else {
                None
            };

            // If not found in orchestrator, check config directly by role or ID
            from_orchestrator.or_else(|| {
                runner.config().orchestrator.specialists.iter()
                    .find(|s| s.role == role_name || s.id == role_name)
                    .map(|spec| AgentConfig {
                        id: spec.id.clone(),
                        name: spec.name.clone(),
                        workspace: spec.workspace.clone(),
                        model: spec.model.clone(),
                        default: false,
                    })
            })
        } else {
            None
        };

        // Build final agent config with overrides
        let final_config = AgentConfig {
            id: task_id.clone(),
            name: agent_config.as_ref().and_then(|c| c.name.clone())
                .or_else(|| role.map(String::from))
                .or(Some(task_id.clone())),
            workspace: workspace_override.map(String::from)
                .or_else(|| agent_config.as_ref().and_then(|c| c.workspace.clone())),
            model: model_override.map(String::from)
                .or_else(|| agent_config.as_ref().and_then(|c| c.model.clone())),
            default: false,
        };

        // Use max_iterations from specialist config if available, otherwise use parameter
        let effective_max_iterations = if agent_config.is_some() && role.is_some() {
            runner.config().orchestrator.specialists.iter()
                .find(|s| s.role == role.unwrap() || s.id == role.unwrap())
                .map(|s| s.max_iterations)
                .unwrap_or(max_iterations)
        } else {
            max_iterations
        };

        tracing::info!(
            "Spawning sub-agent: id={} role={:?} model={:?} workspace={:?} max_iterations={} wait={}",
            task_id,
            role,
            final_config.model,
            final_config.workspace,
            effective_max_iterations,
            wait
        );

        if !wait {
            // Fire-and-forget mode: spawn task in background, return immediately
            let runner_clone = runner.clone();
            let task_owned = task.to_string();
            let final_config_clone = final_config.clone();
            
            tokio::spawn(async move {
                match runner_clone.spawn_agent_with_options(&final_config_clone, effective_max_iterations) {
                    Ok(subagent) => {
                        match runner_clone.process_with_subagent(&subagent, &task_owned, Some(&final_config_clone.id)).await {
                            Ok(result) => {
                                tracing::info!("Background sub-agent {} completed: {} chars", final_config_clone.id, result.len());
                            }
                            Err(e) => {
                                tracing::error!("Background sub-agent {} failed: {}", final_config_clone.id, e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to spawn background sub-agent {}: {}", final_config_clone.id, e);
                    }
                }
            });

            return Ok(ToolResult {
                output: format!(
                    "Sub-agent '{}' spawned in background (fire-and-forget).\n- Task: {}\n- Role: {:?}\n- Model: {:?}\n- Max iterations: {}",
                    task_id,
                    if task.chars().count() > 100 {
                        format!("{}...", task.chars().take(100).collect::<String>())
                    } else {
                        task.to_string()
                    },
                    role,
                    final_config.model,
                    effective_max_iterations
                ),
                is_error: false,
            });
        }

        // Wait mode: spawn and wait for result
        match runner.spawn_agent_with_options(&final_config, effective_max_iterations) {
            Ok(subagent) => {
                match runner.process_with_subagent(&subagent, task, Some(&task_id)).await {
                    Ok(result) => {
                        tracing::info!("Sub-agent {} completed: {} chars", task_id, result.len());
                        Ok(ToolResult {
                            output: format!(
                                "## Sub-agent '{}' completed\n\n### Result:\n{}",
                                task_id, result
                            ),
                            is_error: false,
                        })
                    }
                    Err(e) => {
                        tracing::error!("Sub-agent {} failed: {}", task_id, e);
                        Ok(ToolResult {
                            output: format!("Sub-agent '{}' failed: {}", task_id, e),
                            is_error: true,
                        })
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to spawn sub-agent {}: {}", task_id, e);
                Ok(ToolResult {
                    output: format!("Failed to spawn sub-agent: {}", e),
                    is_error: true,
                })
            }
        }
    }
}

// ─── GID Tools ───────────────────────────────────────────────

type SharedGraph = Arc<tokio::sync::RwLock<Graph>>;
type SharedPath = Arc<String>;

/// Helper: save graph to disk after mutation.
fn save_gid_graph(graph: &Graph, path: &str) -> anyhow::Result<()> {
    gid_save_graph(graph, std::path::Path::new(path))
}

/// Parse status string to NodeStatus.
fn parse_status(s: &str) -> Result<NodeStatus, String> {
    match s {
        "todo" => Ok(NodeStatus::Todo),
        "in_progress" => Ok(NodeStatus::InProgress),
        "done" => Ok(NodeStatus::Done),
        "blocked" => Ok(NodeStatus::Blocked),
        "cancelled" => Ok(NodeStatus::Cancelled),
        _ => Err(format!("Unknown status: {}", s)),
    }
}

// ── gid_tasks: list tasks with optional status filter ──

struct GidTasksTool {
    graph: SharedGraph,
}

impl GidTasksTool {
    fn new(graph: SharedGraph) -> Self {
        Self { graph }
    }
}

#[async_trait]
impl Tool for GidTasksTool {
    fn name(&self) -> &str {
        "gid_tasks"
    }

    fn description(&self) -> &str {
        "List tasks in the project graph. Optionally filter by status. Shows summary stats and ready tasks."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "description": "Filter by status: todo, in_progress, done, blocked, cancelled",
                    "enum": ["todo", "in_progress", "done", "blocked", "cancelled"]
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let graph = self.graph.read().await;
        let status_filter = input["status"].as_str();

        let summary = graph.summary();
        let mut output = format!(
            "📊 Graph: {} nodes, {} edges\n  todo={} in_progress={} done={} blocked={} cancelled={}\n  ready={}\n\n",
            summary.total_nodes, summary.total_edges,
            summary.todo, summary.in_progress, summary.done, summary.blocked, summary.cancelled,
            summary.ready
        );

        let nodes: Vec<&Node> = if let Some(status) = status_filter {
            let target = match parse_status(status) {
                Ok(s) => s,
                Err(e) => return Ok(ToolResult { output: e, is_error: true }),
            };
            graph.nodes.iter().filter(|n| n.status == target).collect()
        } else {
            graph.nodes.iter().collect()
        };

        for node in &nodes {
            let deps: Vec<String> = graph.edges_from(&node.id)
                .iter()
                .filter(|e| e.relation == "depends_on")
                .map(|e| e.to.clone())
                .collect();
            let dep_str = if deps.is_empty() { String::new() } else { format!(" → [{}]", deps.join(", ")) };
            let desc = node.description.as_deref().unwrap_or("");
            let desc_str = if desc.is_empty() { String::new() } else { format!(" — {}", desc) };
            output.push_str(&format!("  [{}] {} ({}){}{}\n", node.status, node.id, node.title, desc_str, dep_str));
        }

        if nodes.is_empty() {
            output.push_str("  (no tasks match)\n");
        }

        Ok(ToolResult { output, is_error: false })
    }
}

// ── gid_add_task: add a new task ──

struct GidAddTaskTool {
    graph: SharedGraph,
    path: SharedPath,
}

impl GidAddTaskTool {
    fn new(graph: SharedGraph, path: SharedPath) -> Self {
        Self { graph, path }
    }
}

#[async_trait]
impl Tool for GidAddTaskTool {
    fn name(&self) -> &str {
        "gid_add_task"
    }

    fn description(&self) -> &str {
        "Add a new task to the project graph."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Unique task ID (e.g. 'impl-auth')" },
                "title": { "type": "string", "description": "Task title" },
                "description": { "type": "string", "description": "Detailed description (optional)" },
                "status": { "type": "string", "enum": ["todo", "in_progress", "done", "blocked"], "description": "Initial status (default: todo)" },
                "priority": { "type": "integer", "description": "Priority 0-255 (0=highest, optional)" },
                "tags": { "type": "array", "items": { "type": "string" }, "description": "Tags (optional)" },
                "depends_on": { "type": "array", "items": { "type": "string" }, "description": "Task IDs this depends on (optional)" }
            },
            "required": ["id", "title"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'id'"))?;
        let title = input["title"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'title'"))?;

        let mut node = Node::new(id, title);

        if let Some(desc) = input["description"].as_str() {
            node = node.with_description(desc);
        }
        if let Some(status) = input["status"].as_str() {
            node = node.with_status(parse_status(status).unwrap_or(NodeStatus::Todo));
        }
        if let Some(p) = input["priority"].as_u64() {
            node = node.with_priority(p as u8);
        }
        if let Some(tags) = input["tags"].as_array() {
            node = node.with_tags(tags.iter().filter_map(|v| v.as_str().map(String::from)).collect());
        }

        let mut graph = self.graph.write().await;
        graph.add_node(node);

        // Add dependency edges
        if let Some(deps) = input["depends_on"].as_array() {
            for dep in deps {
                if let Some(dep_id) = dep.as_str() {
                    graph.add_edge(Edge::depends_on(id, dep_id));
                }
            }
        }

        save_gid_graph(&graph, &self.path)?;

        Ok(ToolResult {
            output: format!("✅ Task '{}' added: {}", id, title),
            is_error: false,
        })
    }
}

// ── gid_update_task: update task status/fields ──

struct GidUpdateTaskTool {
    graph: SharedGraph,
    path: SharedPath,
}

impl GidUpdateTaskTool {
    fn new(graph: SharedGraph, path: SharedPath) -> Self {
        Self { graph, path }
    }
}

#[async_trait]
impl Tool for GidUpdateTaskTool {
    fn name(&self) -> &str {
        "gid_update_task"
    }

    fn description(&self) -> &str {
        "Update a task's status, title, description, or other fields."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Task ID to update" },
                "status": { "type": "string", "enum": ["todo", "in_progress", "done", "blocked", "cancelled"], "description": "New status" },
                "title": { "type": "string", "description": "New title (optional)" },
                "description": { "type": "string", "description": "New description (optional)" }
            },
            "required": ["id"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'id'"))?;

        let mut graph = self.graph.write().await;

        let node = graph.get_node_mut(id)
            .ok_or_else(|| anyhow::anyhow!("Task '{}' not found", id))?;

        let mut changes = Vec::new();

        if let Some(status) = input["status"].as_str() {
            let new_status = match parse_status(status) {
                Ok(s) => s,
                Err(e) => return Ok(ToolResult { output: e, is_error: true }),
            };
            changes.push(format!("status → {}", new_status));
            node.status = new_status;
        }
        if let Some(title) = input["title"].as_str() {
            changes.push(format!("title → {}", title));
            node.title = title.to_string();
        }
        if let Some(desc) = input["description"].as_str() {
            changes.push("description updated".to_string());
            node.description = Some(desc.to_string());
        }

        if changes.is_empty() {
            return Ok(ToolResult { output: format!("No changes for task '{}'", id), is_error: false });
        }

        save_gid_graph(&graph, &self.path)?;

        Ok(ToolResult {
            output: format!("✅ Task '{}' updated: {}", id, changes.join(", ")),
            is_error: false,
        })
    }
}

// ── gid_add_edge: add a dependency edge ──

struct GidAddEdgeTool {
    graph: SharedGraph,
    path: SharedPath,
}

impl GidAddEdgeTool {
    fn new(graph: SharedGraph, path: SharedPath) -> Self {
        Self { graph, path }
    }
}

#[async_trait]
impl Tool for GidAddEdgeTool {
    fn name(&self) -> &str {
        "gid_add_edge"
    }

    fn description(&self) -> &str {
        "Add a dependency edge between two tasks (e.g. task A depends_on task B)."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "from": { "type": "string", "description": "Source task ID" },
                "to": { "type": "string", "description": "Target task ID" },
                "relation": { "type": "string", "enum": ["depends_on", "blocks", "subtask_of", "relates_to"], "description": "Relationship type (default: depends_on)" }
            },
            "required": ["from", "to"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let from = input["from"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'from'"))?;
        let to = input["to"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'to'"))?;
        let relation = input["relation"].as_str().unwrap_or("depends_on");

        let mut graph = self.graph.write().await;
        graph.add_edge(Edge::new(from, to, relation));
        save_gid_graph(&graph, &self.path)?;

        Ok(ToolResult {
            output: format!("✅ Edge added: {} —[{}]→ {}", from, relation, to),
            is_error: false,
        })
    }
}

// ── gid_read: read full graph as YAML ──

struct GidReadTool {
    graph: SharedGraph,
}

impl GidReadTool {
    fn new(graph: SharedGraph) -> Self {
        Self { graph }
    }
}

#[async_trait]
impl Tool for GidReadTool {
    fn name(&self) -> &str {
        "gid_read"
    }

    fn description(&self) -> &str {
        "Read the full project graph as YAML (nodes, edges, dependencies)."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _input: Value) -> anyhow::Result<ToolResult> {
        let graph = self.graph.read().await;
        let yaml = serde_yaml::to_string(&*graph)?;
        Ok(ToolResult { output: yaml, is_error: false })
    }
}

// ── gid_complete: mark task done and show unblocked tasks ──

struct GidCompleteTool {
    graph: SharedGraph,
    path: SharedPath,
}

impl GidCompleteTool {
    fn new(graph: SharedGraph, path: SharedPath) -> Self {
        Self { graph, path }
    }
}

#[async_trait]
impl Tool for GidCompleteTool {
    fn name(&self) -> &str {
        "gid_complete"
    }

    fn description(&self) -> &str {
        "Mark a task as done and show which tasks are now unblocked."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Task ID to mark as done" }
            },
            "required": ["id"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'id'"))?;

        let mut graph = self.graph.write().await;

        // Check task exists
        if graph.get_node(id).is_none() {
            return Ok(ToolResult { output: format!("Task '{}' not found", id), is_error: true });
        }

        // Get ready tasks before completion
        let ready_before: std::collections::HashSet<String> = graph.ready_tasks()
            .iter().map(|n| n.id.clone()).collect();

        // Mark done
        graph.update_status(id, NodeStatus::Done);

        // Get ready tasks after completion
        let ready_after: Vec<&Node> = graph.ready_tasks();
        let newly_unblocked: Vec<&str> = ready_after.iter()
            .filter(|n| !ready_before.contains(&n.id))
            .map(|n| n.id.as_str())
            .collect();

        save_gid_graph(&graph, &self.path)?;

        let mut output = format!("✅ Task '{}' marked done.", id);
        if !newly_unblocked.is_empty() {
            output.push_str(&format!("\n🔓 Now unblocked: {}", newly_unblocked.join(", ")));
        }

        Ok(ToolResult { output, is_error: false })
    }
}

// ── gid_query_impact: impact analysis ──

struct GidQueryImpactTool {
    graph: SharedGraph,
}

impl GidQueryImpactTool {
    fn new(graph: SharedGraph) -> Self {
        Self { graph }
    }
}

#[async_trait]
impl Tool for GidQueryImpactTool {
    fn name(&self) -> &str {
        "gid_query_impact"
    }

    fn description(&self) -> &str {
        "Analyze impact: what tasks would be affected if this task changes?"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Task ID to analyze" }
            },
            "required": ["id"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'id'"))?;

        let graph = self.graph.read().await;
        let engine = QueryEngine::new(&graph);

        let impacted = engine.impact(id);

        if impacted.is_empty() {
            return Ok(ToolResult {
                output: format!("No other tasks depend on '{}'.", id),
                is_error: false,
            });
        }

        let mut output = format!("🔥 {} task(s) would be impacted by changes to '{}':\n", impacted.len(), id);
        for node in impacted {
            output.push_str(&format!("  • {} ({})\n", node.id, node.title));
        }

        Ok(ToolResult { output, is_error: false })
    }
}

// ── gid_query_deps: dependency query ──

struct GidQueryDepsTool {
    graph: SharedGraph,
}

impl GidQueryDepsTool {
    fn new(graph: SharedGraph) -> Self {
        Self { graph }
    }
}

#[async_trait]
impl Tool for GidQueryDepsTool {
    fn name(&self) -> &str {
        "gid_query_deps"
    }

    fn description(&self) -> &str {
        "Query dependencies: what does this task depend on (direct or transitive)?"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Task ID to query" },
                "transitive": { "type": "boolean", "description": "Include transitive dependencies (default: true)" }
            },
            "required": ["id"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'id'"))?;
        let transitive = input["transitive"].as_bool().unwrap_or(true);

        let graph = self.graph.read().await;
        let engine = QueryEngine::new(&graph);

        let deps = engine.deps(id, transitive);

        if deps.is_empty() {
            return Ok(ToolResult {
                output: format!("Task '{}' has no dependencies.", id),
                is_error: false,
            });
        }

        let mut output = format!("📦 {} dependencies for '{}':\n", deps.len(), id);
        for node in deps {
            let status_icon = match node.status {
                NodeStatus::Done => "✅",
                NodeStatus::InProgress => "🔄",
                _ => "○",
            };
            output.push_str(&format!("  {} {} ({})\n", status_icon, node.id, node.title));
        }

        Ok(ToolResult { output, is_error: false })
    }
}

// ── gid_validate: graph validation ──

struct GidValidateTool {
    graph: SharedGraph,
}

impl GidValidateTool {
    fn new(graph: SharedGraph) -> Self {
        Self { graph }
    }
}

#[async_trait]
impl Tool for GidValidateTool {
    fn name(&self) -> &str {
        "gid_validate"
    }

    fn description(&self) -> &str {
        "Validate graph integrity: detect cycles, orphan nodes, missing references."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _input: Value) -> anyhow::Result<ToolResult> {
        let graph = self.graph.read().await;
        let validator = Validator::new(&graph);
        let result = validator.validate();

        Ok(ToolResult {
            output: result.to_string(),
            is_error: !result.is_valid(),
        })
    }
}

// ── gid_advise: graph analysis and suggestions ──

struct GidAdviseTool {
    graph: SharedGraph,
}

impl GidAdviseTool {
    fn new(graph: SharedGraph) -> Self {
        Self { graph }
    }
}

#[async_trait]
impl Tool for GidAdviseTool {
    fn name(&self) -> &str {
        "gid_advise"
    }

    fn description(&self) -> &str {
        "Analyze graph and suggest improvements: detect issues, recommend task order."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _input: Value) -> anyhow::Result<ToolResult> {
        let graph = self.graph.read().await;
        let result = advise_analyze(&graph);

        if result.items.is_empty() {
            return Ok(ToolResult {
                output: format!("✅ No issues found. Graph looks healthy! Score: {}/100", result.health_score),
                is_error: false,
            });
        }

        let mut output = format!("📋 {} suggestion(s) (score: {}/100):\n\n", result.items.len(), result.health_score);
        for advice in &result.items {
            output.push_str(&format!("{}\n\n", advice));
        }

        Ok(ToolResult { output, is_error: false })
    }
}

// ── gid_visual: render graph as ASCII ──

struct GidVisualTool {
    graph: SharedGraph,
}

impl GidVisualTool {
    fn new(graph: SharedGraph) -> Self {
        Self { graph }
    }
}

#[async_trait]
impl Tool for GidVisualTool {
    fn name(&self) -> &str {
        "gid_visual"
    }

    fn description(&self) -> &str {
        "Render the graph visually (ASCII tree, DOT, or Mermaid diagram)."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "format": {
                    "type": "string",
                    "enum": ["ascii", "dot", "mermaid"],
                    "description": "Output format (default: ascii)"
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let format_str = input["format"].as_str().unwrap_or("ascii");
        let format = match format_str.parse::<VisualFormat>() {
            Ok(f) => f,
            Err(e) => return Ok(ToolResult { output: e.to_string(), is_error: true }),
        };

        let graph = self.graph.read().await;
        let output = render(&graph, format);

        Ok(ToolResult { output, is_error: false })
    }
}

// ── gid_history: list/save snapshots ──

struct GidHistoryTool {
    path: SharedPath,
}

impl GidHistoryTool {
    fn new(path: SharedPath) -> Self {
        Self { path }
    }
}

#[async_trait]
impl Tool for GidHistoryTool {
    fn name(&self) -> &str {
        "gid_history"
    }

    fn description(&self) -> &str {
        "List graph history snapshots or save a new snapshot."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "save"],
                    "description": "Action to perform (default: list)"
                },
                "message": {
                    "type": "string",
                    "description": "Commit message when saving (optional)"
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let action = input["action"].as_str().unwrap_or("list");
        let graph_path = std::path::Path::new(self.path.as_str());
        let history_dir = graph_path.parent()
            .unwrap_or(std::path::Path::new("."))
            .join(".gid-history");

        let manager = HistoryManager::new(&history_dir);

        match action {
            "list" => {
                let entries = manager.list_snapshots()?;
                if entries.is_empty() {
                    return Ok(ToolResult {
                        output: "No history snapshots found.".to_string(),
                        is_error: false,
                    });
                }
                let mut output = format!("📜 {} snapshot(s):\n", entries.len());
                for entry in entries.iter().take(10) {
                    let msg = entry.message.as_deref().unwrap_or("-");
                    output.push_str(&format!("  {} — {} ({} nodes, {} edges)\n",
                        entry.timestamp, msg, entry.node_count, entry.edge_count));
                }
                if entries.len() > 10 {
                    output.push_str(&format!("  ... and {} more\n", entries.len() - 10));
                }
                Ok(ToolResult { output, is_error: false })
            }
            "save" => {
                let message = input["message"].as_str();
                let graph = gid_load_graph(graph_path)?;
                let filename = manager.save_snapshot(&graph, message)?;
                Ok(ToolResult {
                    output: format!("📸 Snapshot saved: {}", filename),
                    is_error: false,
                })
            }
            _ => Ok(ToolResult {
                output: format!("Unknown action: {}. Use 'list' or 'save'.", action),
                is_error: true,
            }),
        }
    }
}

// ── gid_refactor: rename/merge/split nodes ──

struct GidRefactorTool {
    graph: SharedGraph,
    path: SharedPath,
}

impl GidRefactorTool {
    fn new(graph: SharedGraph, path: SharedPath) -> Self {
        Self { graph, path }
    }
}

#[async_trait]
impl Tool for GidRefactorTool {
    fn name(&self) -> &str {
        "gid_refactor"
    }

    fn description(&self) -> &str {
        "Refactor graph structure: rename nodes, merge tasks, split tasks, update titles."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["rename", "merge", "update_title"],
                    "description": "Operation type"
                },
                "id": { "type": "string", "description": "Target node ID" },
                "new_id": { "type": "string", "description": "New ID (for rename)" },
                "new_title": { "type": "string", "description": "New title (for update_title)" },
                "merge_into": { "type": "string", "description": "Target node to merge into (for merge)" },
                "preview": { "type": "boolean", "description": "Preview only, don't apply (default: false)" }
            },
            "required": ["operation", "id"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let operation = input["operation"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'operation'"))?;
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'id'"))?;
        let preview = input["preview"].as_bool().unwrap_or(false);

        let mut graph = self.graph.write().await;

        match operation {
            "rename" => {
                let new_id = input["new_id"].as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'new_id' for rename"))?;

                if preview {
                    return match refactor::preview_rename(&graph, id, new_id) {
                        Some(p) => Ok(ToolResult { output: p.to_string(), is_error: false }),
                        None => Ok(ToolResult { output: format!("Node '{}' not found", id), is_error: true }),
                    };
                }

                if !refactor::apply_rename(&mut graph, id, new_id) {
                    return Ok(ToolResult {
                        output: format!("Failed to rename: '{}' not found or '{}' already exists", id, new_id),
                        is_error: true,
                    });
                }
                save_gid_graph(&graph, &self.path)?;

                Ok(ToolResult {
                    output: format!("✅ Renamed '{}' → '{}'", id, new_id),
                    is_error: false,
                })
            }
            "merge" => {
                let target = input["merge_into"].as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'merge_into' for merge"))?;
                // Generate new merged ID
                let new_id = format!("{}-{}", id, target);

                if preview {
                    return match refactor::preview_merge(&graph, id, target, &new_id) {
                        Some(p) => Ok(ToolResult { output: p.to_string(), is_error: false }),
                        None => Ok(ToolResult { output: format!("One or both nodes not found: '{}', '{}'", id, target), is_error: true }),
                    };
                }

                if !refactor::apply_merge(&mut graph, id, target, &new_id) {
                    return Ok(ToolResult {
                        output: format!("Failed to merge: one or both nodes not found"),
                        is_error: true,
                    });
                }
                save_gid_graph(&graph, &self.path)?;

                Ok(ToolResult {
                    output: format!("✅ Merged '{}' + '{}' → '{}'", id, target, new_id),
                    is_error: false,
                })
            }
            "update_title" => {
                let new_title = input["new_title"].as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'new_title'"))?;

                if !refactor::update_title(&mut graph, id, new_title) {
                    return Ok(ToolResult {
                        output: format!("Node '{}' not found", id),
                        is_error: true,
                    });
                }
                save_gid_graph(&graph, &self.path)?;

                Ok(ToolResult {
                    output: format!("✅ Updated title for '{}': {}", id, new_title),
                    is_error: false,
                })
            }
            _ => Ok(ToolResult {
                output: format!("Unknown operation: {}. Use 'rename', 'merge', or 'update_title'.", operation),
                is_error: true,
            }),
        }
    }
}
