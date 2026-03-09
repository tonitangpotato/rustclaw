//! Tool system — exec, file operations, web fetch, memory.
//!
//! Tools are registered in a registry and dispatched by the agent loop.
//! Each tool implements the Tool trait and provides its JSON schema for LLM.

use std::sync::Arc;

use async_trait::async_trait;
use engramai::MemoryType;
use serde::Serialize;
use serde_json::Value;

use crate::memory::MemoryManager;
use crate::orchestrator::{SharedOrchestrator, Task};

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
    
    /// Register all default tools including memory tools.
    pub fn with_defaults_and_memory(workspace_root: &str, memory: Arc<MemoryManager>) -> Self {
        let mut registry = Self::with_defaults(workspace_root);
        registry.register(Box::new(EngramRecallTool::new(memory.clone())));
        registry.register(Box::new(EngramStoreTool::new(memory)));
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
                &result[..50_000],
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
            format!("{}\n... (truncated)", &text[..max_chars])
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
            format!("{}\n... (truncated, too many matches)", &stdout[..30_000])
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
                    if content.len() > 50 {
                        format!("{}...", &content[..50])
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

// ─── Delegate Task Tool ──────────────────────────────────────

/// Delegate a task to a specialist agent via the orchestrator.
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
        "Delegate a task to a specialist agent. The task will be queued and assigned to an appropriate agent based on role matching. Use this for complex or time-consuming tasks that can be handled by specialist agents (e.g., builder, visibility, trading)."
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

        Ok(ToolResult {
            output: format!(
                "Task '{}' submitted to orchestrator.\n- Description: {}\n- Roles: {:?}\n- Priority: {}\nThe task will be assigned to an appropriate specialist agent.",
                task_id,
                if description.len() > 100 {
                    format!("{}...", &description[..100])
                } else {
                    description.to_string()
                },
                if roles.is_empty() { vec!["any".to_string()] } else { roles },
                priority
            ),
            is_error: false,
        })
    }
}
