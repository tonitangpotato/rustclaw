//! Tool system — exec, file operations, web fetch.
//!
//! Tools are registered in a registry and dispatched by the agent loop.
//! Each tool implements the Tool trait and provides its JSON schema for LLM.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
