//! Tool system — exec, file operations, web fetch, memory.
//!
//! Tools are registered in a registry and dispatched by the agent loop.
//! Each tool implements the Tool trait and provides its JSON schema for LLM.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use engramai::MemoryType;
use serde::Serialize;
use serde_json::Value;

use gid_core::{
    Graph, Node, Edge, NodeStatus,
    parser::{load_graph as gid_load_graph, save_graph as gid_save_graph},
    storage::{load_graph_auto, save_graph_auto, StorageBackend, detect_backend},
    query::QueryEngine,
    validator::Validator,
    visual::{render, VisualFormat},
    advise::{analyze as advise_analyze},
    history::HistoryManager,
    refactor,
    CodeGraph,
    unify::{codegraph_to_graph_nodes, merge_code_layer, graph_to_codegraph},
    semantify,
    complexity,
    working_mem,
    ignore,
    infer::{self, InferConfig, InferLevel, OutputFormat as InferOutputFormat, merge_into_graph, format_output as infer_format_output, rollback_infer_batch},
    ritual::scope::{default_scope_for_phase, ToolScope},
    harness::{
        assemble_task_context,
        ContextQuery, ContextFilters, OutputFormat as ContextOutputFormat,
        assemble_context, format_context,
    },
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
    llm_client: Option<Arc<tokio::sync::RwLock<Box<dyn crate::llm::LlmClient>>>>,
    /// Memory manager stored for late-binding tool registration (e.g. KnowledgeCompileTool
    /// which needs the LLM client but is constructed before `set_llm_client()` is called).
    memory: Option<Arc<MemoryManager>>,
    workspace_root: Option<std::path::PathBuf>,
    /// Shared mutable slot for ritual notify — set per-request with chat context,
    /// read by StartRitualTool at execution time.
    pub ritual_notify: Arc<std::sync::Mutex<Option<crate::ritual_runner::NotifyFn>>>,
    /// Shared slot for the ritual registry. Populated late (by `Agent::new()`
    /// after it builds the registry from config), read by StartRitualTool on
    /// each invocation to invalidate the cache.
    pub ritual_registry: Arc<std::sync::Mutex<Option<Arc<crate::ritual_registry::RitualRegistry>>>>,
    /// Current parent session key — set per-request by telegram.rs,
    /// used by fire-and-forget sub-agents to inject completion back into parent.
    pub current_session_key: Arc<std::sync::Mutex<Option<String>>>,
    /// Broadcast sender for sub-agent lifecycle events.
    pub subagent_event_tx: Option<tokio::sync::broadcast::Sender<crate::events::SubAgentEvent>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: Vec::new(),
            llm_client: None,
            memory: None,
            workspace_root: None,
            ritual_notify: Arc::new(std::sync::Mutex::new(None)),
            ritual_registry: Arc::new(std::sync::Mutex::new(None)),
            current_session_key: Arc::new(std::sync::Mutex::new(None)),
            subagent_event_tx: None,
        }
    }

    /// Set the LLM client for ritual tools. Must be called after `with_defaults_and_memory`
    /// (or equivalent) — this is the point at which late-bound tools like
    /// `KnowledgeCompileTool` are actually registered (they need both memory and LLM).
    pub fn set_llm_client(&mut self, client: Arc<tokio::sync::RwLock<Box<dyn crate::llm::LlmClient>>>) {
        self.llm_client = Some(client.clone());
        // Late-bind KnowledgeCompileTool now that both memory and LLM are available.
        if let Some(mem) = self.memory.clone() {
            self.register(Box::new(KnowledgeCompileTool::new(mem, client)));
        }
    }



    /// Register all default tools.
    pub fn with_defaults(workspace_root: &str, config: &crate::config::Config) -> Self {
        let mut registry = Self::new();
        registry.workspace_root = Some(std::path::PathBuf::from(workspace_root));
        registry.register(Box::new(ExecTool));
        registry.register(Box::new(ReadFileTool::new(workspace_root)));
        registry.register(Box::new(WriteFileTool::new(workspace_root)));
        registry.register(Box::new(ListDirTool::new(workspace_root)));
        registry.register(Box::new(WebFetchTool));
        registry.register(Box::new(EditFileTool::new(workspace_root)));
        registry.register(Box::new(SearchFilesTool::new(workspace_root)));
        registry.register(Box::new(RestartSelfTool));
        // Web search (requires Brave API key)
        if let Some(key) = &config.web_search.brave_api_key {
            registry.register(Box::new(WebSearchTool::new(key.clone())));
        }
        registry
    }

    /// Register core tools for sub-agents (no GID, no orchestrator tools).
    /// Tools are scoped to the given workspace root.
    pub fn for_subagent(workspace_root: &str) -> Self {
        let mut registry = Self::new();
        registry.workspace_root = Some(std::path::PathBuf::from(workspace_root));
        registry.register(Box::new(ExecTool));
        registry.register(Box::new(ReadFileTool::new(workspace_root)));
        registry.register(Box::new(WriteFileTool::new(workspace_root)));
        registry.register(Box::new(ListDirTool::new(workspace_root)));
        registry.register(Box::new(EditFileTool::new(workspace_root)));
        registry.register(Box::new(SearchFilesTool::new(workspace_root)));
        registry.register(Box::new(WebFetchTool));
        registry.register(Box::new(TtsTool));
        registry.register(Box::new(SttTool));
        registry
    }
    


    /// Register core tools for sub-agents with shared memory (engram).
    /// Sub-agents share the main memory manager for cross-agent memory access.
    /// The agent_id parameter sets the namespace for engram operations.
    pub fn for_subagent_with_memory(workspace_root: &str, memory: Arc<MemoryManager>) -> Self {
        let mut registry = Self::for_subagent(workspace_root);
        // Add engram tools with the shared memory manager
        registry.register(Box::new(EngramRecallTool::new(memory.clone())));
        registry.register(Box::new(EngramStoreTool::new(memory.clone())));
        registry.register(Box::new(EngramRecallAssociatedTool::new(memory)));
        registry
    }
    
    /// Register all default tools including memory tools.
    pub fn with_defaults_and_memory(workspace_root: &str, memory: Arc<MemoryManager>, config: &crate::config::Config) -> Self {
        let mut registry = Self::with_defaults(workspace_root, config);
        registry.register(Box::new(EngramRecallTool::new(memory.clone())));
        registry.register(Box::new(EngramStoreTool::new(memory.clone())));
        registry.register(Box::new(EngramRecallAssociatedTool::new(memory.clone())));
        // EmpathyBus tools for introspection
        registry.register(Box::new(EngramTrendsTool::new(memory.clone())));
        registry.register(Box::new(EngramBehaviorStatsTool::new(memory.clone())));
        registry.register(Box::new(EngramSoulSuggestionsTool::new(memory.clone())));
        // Knowledge Compiler tools
        registry.register(Box::new(KnowledgeQueryTool::new(memory.clone())));
        registry.register(Box::new(KnowledgeListTool::new(memory.clone())));
        registry.register(Box::new(KnowledgeHealthTool::new(memory.clone())));
        // KnowledgeCompileTool is late-bound in `set_llm_client()` once the LLM client
        // is available — it needs both memory and LLM, and we avoid wrapping in Option.
        registry.memory = Some(memory);
        // TTS and STT tools
        registry.register(Box::new(TtsTool));
        registry.register(Box::new(SttTool));
        registry
    }

    /// Register all default tools including memory and orchestrator tools.
    pub fn with_defaults_and_orchestrator(
        workspace_root: &str,
        memory: Arc<MemoryManager>,
        orchestrator: SharedOrchestrator,
        config: &crate::config::Config,
    ) -> Self {
        let mut registry = Self::with_defaults_and_memory(workspace_root, memory, config);
        registry.register(Box::new(DelegateTaskTool::new(orchestrator)));
        registry
    }

    /// Register the spawn_specialist tool with access to the agent runner.
    /// The runner handle is initially empty and must be set after AgentRunner creation.
    pub fn with_spawn_specialist(mut self, runner: SharedAgentRunner, orchestrator: Option<SharedOrchestrator>) -> Self {
        let notify_slot = self.ritual_notify.clone();
        let session_key_slot = self.current_session_key.clone();
        let event_tx = self.subagent_event_tx.clone();
        self.register(Box::new(SpawnSpecialistTool::new(runner, orchestrator, notify_slot, session_key_slot, event_tx)));
        self
    }

    /// Register GID (task graph) tools.
    /// All GID tools receive a shared GraphManager that supports cross-project access
    /// via the optional `"project"` parameter.
    pub fn with_gid(mut self, graph_path: &str) -> Self {
        let mgr = Arc::new(GraphManager::new(graph_path));

        // Original 5 tools (backward compatible)
        self.register(Box::new(GidTasksTool { mgr: mgr.clone() }));
        self.register(Box::new(GidAddTaskTool { mgr: mgr.clone() }));
        self.register(Box::new(GidUpdateTaskTool { mgr: mgr.clone() }));
        self.register(Box::new(GidAddEdgeTool { mgr: mgr.clone() }));
        self.register(Box::new(GidReadTool { mgr: mgr.clone() }));

        // New gid-core tools
        self.register(Box::new(GidCompleteTool { mgr: mgr.clone() }));
        self.register(Box::new(GidQueryImpactTool { mgr: mgr.clone() }));
        self.register(Box::new(GidQueryDepsTool { mgr: mgr.clone() }));
        self.register(Box::new(GidValidateTool { mgr: mgr.clone() }));
        self.register(Box::new(GidAdviseTool { mgr: mgr.clone() }));
        self.register(Box::new(GidVisualTool { mgr: mgr.clone() }));
        self.register(Box::new(GidHistoryTool { mgr: mgr.clone() }));
        self.register(Box::new(GidRefactorTool { mgr: mgr.clone() }));

        // Code graph extraction tools
        self.register(Box::new(GidExtractTool { mgr: mgr.clone() }));
        self.register(Box::new(GidSchemaTool { mgr: mgr.clone() }));
        // Design, planning, ritual, and execution tools
        self.register(Box::new(GidDesignTool { mgr: mgr.clone() }));
        self.register(Box::new(GidPlanTool { mgr: mgr.clone() }));
        // V2 ritual: single tool for LLM to trigger ritual programmatically
        self.register(Box::new(StartRitualTool::new(
            self.workspace_root.clone().unwrap_or_else(|| mgr.workspace_gid_dir.parent().unwrap_or(std::path::Path::new(".")).to_path_buf()),
            self.llm_client.clone(),
            self.ritual_notify.clone(),
            self.ritual_registry.clone(),
        )));

        self.register(Box::new(GidStatsTool { mgr: mgr.clone() }));

        // Additional gid-core tools (semantify, complexity, working memory, ignore, scope)
        self.register(Box::new(GidSemantifyTool { mgr: mgr.clone() }));
        self.register(Box::new(GidComplexityTool));
        self.register(Box::new(GidWorkingMemoryTool));
        self.register(Box::new(GidIgnoreTool { mgr: mgr.clone() }));
        self.register(Box::new(GidScopeTool));
        self.register(Box::new(GidInferTool { mgr: mgr.clone(), llm_client: self.llm_client.clone() }));

        // Context assembly tools
        self.register(Box::new(GidContextTool { mgr: mgr.clone() }));
        self.register(Box::new(GidTaskContextTool { mgr: mgr.clone() }));

        self
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        tracing::debug!("Registered tool: {}", tool.name());
        self.tools.push(tool);
    }

    /// Get LLM tool definitions for all registered tools.
    pub fn definitions(&self) -> Vec<crate::llm::ToolDefinition> {
        let mut defs: Vec<_> = self.tools
            .iter()
            .map(|t| crate::llm::ToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                input_schema: t.input_schema(),
            })
            .collect();

        // Virtual tool: set_voice_mode (intercepted by agent, not in registry)
        defs.push(crate::llm::ToolDefinition {
            name: "set_voice_mode".to_string(),
            description: "Toggle voice mode for the current chat. When enabled, all replies are automatically converted to voice messages via TTS.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "enabled": {
                        "type": "boolean",
                        "description": "true to enable voice replies, false to disable"
                    }
                },
                "required": ["enabled"]
            }),
        });

        defs
    }

    /// Execute a tool by name.
    /// Applies tool gating: source code writes and build commands are blocked
    /// unless a ritual is active, forcing the agent through the ritual pipeline.
    pub async fn execute(&self, name: &str, input: Value) -> anyhow::Result<ToolResult> {
        // ── Tool gating check ──
        if let Some(ref workspace) = self.workspace_root {
            let config = gid_core::ritual::load_gating_config(workspace);
            if config.enabled {
                let ritual_active = self.is_ritual_active(workspace);

                // Normalize path to relative (LLM may pass absolute paths)
                let raw_path = input["path"].as_str()
                    .or_else(|| input["file_path"].as_str());
                let file_path = raw_path.map(|p| {
                    let workspace_str = workspace.to_str().unwrap_or("");
                    if !workspace_str.is_empty() && p.starts_with(workspace_str) {
                        // Strip workspace prefix: /Users/potato/rustclaw/src/foo.rs → src/foo.rs
                        p.strip_prefix(workspace_str)
                            .unwrap_or(p)
                            .trim_start_matches('/')
                    } else {
                        p
                    }
                });
                let command = input["command"].as_str();

                tracing::debug!(
                    tool = name,
                    path = ?file_path,
                    command = ?command,
                    ritual_active = ritual_active,
                    "Tool gating check"
                );

                let result = gid_core::ritual::check_gating(
                    &config, name, file_path, command, ritual_active,
                );

                match &result {
                    gid_core::ritual::GatingResult::Blocked { reason } => {
                        tracing::warn!(tool = name, path = ?file_path, "Tool gating BLOCKED: {}", reason);
                        return Ok(ToolResult {
                            output: reason.clone(),
                            is_error: true,
                        });
                    }
                    gid_core::ritual::GatingResult::Allowed => {
                        // Only log for write tools to avoid noise
                        if matches!(name, "write_file" | "edit_file" | "exec") {
                            tracing::debug!(tool = name, path = ?file_path, "Tool gating ALLOWED");
                        }
                    }
                }
            } else {
                tracing::debug!("Tool gating DISABLED in config");
            }
        }

        let tool = self
            .tools
            .iter()
            .find(|t| t.name() == name)
            .ok_or_else(|| anyhow::anyhow!("Unknown tool: {}", name))?;

        tool.execute(input).await
    }

    /// Check if a ritual is currently active by reading .gid/ritual-state.json.
    fn is_ritual_active(&self, workspace: &std::path::Path) -> bool {
        let state_path = workspace.join(".gid").join("ritual-state.json");
        if !state_path.exists() {
            return false;
        }
        match std::fs::read_to_string(&state_path) {
            Ok(content) => {
                // Check if phase is non-terminal (not Idle/Done/Escalated/Cancelled)
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                    let phase = v["phase"].as_str().unwrap_or("idle");
                    !matches!(phase, "idle" | "done" | "escalated" | "cancelled")
                } else {
                    false
                }
            }
            Err(_) => false,
        }
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

        // Truncate if too long (20K chars for bash output)
        if result.len() > 20_000 {
            // Find a valid UTF-8 char boundary at or before 20_000
            let mut end = 20_000;
            while !result.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            result.truncate(end);
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

// ─── Restart Self Tool ──────────────────────────────────────

/// Safely restart this RustClaw process via clean exit.
/// launchd KeepAlive will respawn automatically.
pub struct RestartSelfTool;

#[async_trait]
impl Tool for RestartSelfTool {
    fn name(&self) -> &str {
        "restart_self"
    }

    fn description(&self) -> &str {
        "Safely restart this RustClaw instance. The process exits cleanly and launchd respawns it automatically. Use this instead of kill/launchctl commands."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "reason": {
                    "type": "string",
                    "description": "Why the restart is needed (logged before exit)"
                }
            },
            "required": ["reason"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let reason = input["reason"].as_str().unwrap_or("no reason given");
        tracing::info!("Restart requested: {}", reason);

        // Notify Telegram BEFORE exiting so potato knows we're coming back.
        // Best-effort; no-op if no telegram config loaded.
        crate::lifecycle::broadcast(&format!(
            "🔄 Restarting in 2s: {}\nWill notify when back up.",
            reason
        ));

        // Write clean shutdown marker with restart: prefix so startup
        // detection can distinguish this from SIGTERM and emit "Reborn" message.
        crate::lifecycle::mark_shutdown(&format!("restart:{}", reason), true);

        // Spawn a delayed exit so the tool result can be sent back first
        tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            tracing::info!("Exiting for restart (KeepAlive will respawn)");
            std::process::exit(0);
        });

        Ok(ToolResult {
            output: format!("Restarting in 2 seconds (reason: {}). I'll be back shortly.", reason),
            is_error: false,
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
        let limit = input["limit"].as_u64().unwrap_or(500) as usize;

        let selected: Vec<&str> = lines.iter().skip(offset).take(limit).copied().collect();
        let result = selected.join("\n");

        // Truncate if too long (15K chars ≈ 3750 tokens)
        let result = if result.len() > 15_000 {
            format!(
                "{}\n... (truncated, {} total lines. Use offset/limit to read more.)",
                crate::text_utils::truncate_bytes(&result, 15_000),
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

        // Post-write syntax validation for supported languages
        let mut result_msg = format!("Wrote {} bytes to {}", content.len(), path.display());
        
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let check_cmd = match ext {
                "rs" => {
                    // Find Cargo.toml by walking up
                    let mut cargo_dir = path.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
                    loop {
                        if cargo_dir.join("Cargo.toml").exists() {
                            break;
                        }
                        if !cargo_dir.pop() {
                            break;
                        }
                    }
                    if cargo_dir.join("Cargo.toml").exists() {
                        Some(format!(
                            "cd {} && cargo check --message-format=short 2>&1 | head -20",
                            cargo_dir.display()
                        ))
                    } else {
                        None
                    }
                }
                "py" => Some(format!("python3 -c \"import ast; ast.parse(open('{}').read())\" 2>&1", path.display())),
                "ts" | "tsx" => Some(format!("npx tsc --noEmit {} 2>&1 | head -10", path.display())),
                "js" | "jsx" => Some(format!("node --check {} 2>&1", path.display())),
                "json" => Some(format!("python3 -c \"import json; json.load(open('{}'))\" 2>&1", path.display())),
                "yaml" | "yml" => Some(format!("python3 -c \"import yaml; yaml.safe_load(open('{}'))\" 2>&1", path.display())),
                _ => None,
            };
            
            if let Some(cmd) = check_cmd {
                if let Ok(output) = tokio::process::Command::new("sh")
                    .args(["-c", &cmd])
                    .output()
                    .await
                {
                    if !output.status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let msg = if stderr.is_empty() { stdout } else { stderr };
                        result_msg.push_str(&format!("\n\n⚠️ Post-write validation warning:\n{}", msg.trim()));
                    }
                }
            }
        }

        Ok(ToolResult {
            output: result_msg,
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
                    "description": "Maximum characters to return (default: 15000)"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let url = input["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'url' parameter"))?;
        let max_chars = input["max_chars"].as_u64().unwrap_or(15_000) as usize;

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

// ─── Web Search Tool ─────────────────────────────────────────

/// Web search via Brave Search API.
pub struct WebSearchTool {
    api_key: String,
}

impl WebSearchTool {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web using Brave Search. Returns titles, URLs, and snippets."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query string"
                },
                "count": {
                    "type": "integer",
                    "description": "Number of results (1-10, default: 5)"
                },
                "freshness": {
                    "type": "string",
                    "description": "Time filter: 'day', 'week', 'month', or 'year'"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let query = input["query"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' parameter"))?;
        let count = input["count"].as_u64().unwrap_or(5).min(10).max(1);
        let freshness = input["freshness"].as_str();

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;

        let mut req = client
            .get("https://api.search.brave.com/res/v1/web/search")
            .header("Accept", "application/json")
            .header("Accept-Encoding", "gzip")
            .header("X-Subscription-Token", &self.api_key)
            .query(&[("q", query), ("count", &count.to_string())]);

        if let Some(f) = freshness {
            req = req.query(&[("freshness", f)]);
        }

        let resp = req.send().await?;
        let status = resp.status();

        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Ok(ToolResult {
                output: format!("Brave API error {}: {}", status, body),
                is_error: true,
            });
        }

        let body: Value = resp.json().await?;

        let mut results = Vec::new();
        if let Some(web) = body["web"].as_object() {
            if let Some(items) = web["results"].as_array() {
                for item in items.iter().take(count as usize) {
                    let title = item["title"].as_str().unwrap_or("");
                    let url = item["url"].as_str().unwrap_or("");
                    let desc = item["description"].as_str().unwrap_or("");
                    results.push(format!("**{}**\n{}\n{}", title, url, desc));
                }
            }
        }

        if results.is_empty() {
            Ok(ToolResult {
                output: "No results found.".to_string(),
                is_error: false,
            })
        } else {
            Ok(ToolResult {
                output: results.join("\n\n"),
                is_error: false,
            })
        }
    }
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

    /// Attempt fuzzy match when exact match fails.
    /// Tries: whitespace normalization, leading/trailing trim, line-by-line fuzzy.
    /// Returns the exact substring from the file that best matches, or None.
    fn fuzzy_find<'a>(content: &'a str, target: &str) -> Option<&'a str> {
        // Strategy 1: Normalize whitespace (collapse runs of whitespace to single space)
        let normalize = |s: &str| -> String {
            s.split_whitespace().collect::<Vec<_>>().join(" ")
        };
        let target_normalized = normalize(target);

        // Find by normalized comparison against sliding windows of lines
        let content_lines: Vec<&str> = content.lines().collect();
        let target_lines: Vec<&str> = target.lines().collect();
        let target_line_count = target_lines.len();

        if target_line_count == 0 || content_lines.is_empty() {
            return None;
        }

        let mut best_match: Option<(usize, usize, f64)> = None; // (start_line, end_line, score)

        for start in 0..=content_lines.len().saturating_sub(target_line_count) {
            let end = (start + target_line_count).min(content_lines.len());
            let window: Vec<&str> = content_lines[start..end].to_vec();

            // Score: ratio of matching normalized lines
            let mut matching = 0;
            for (wline, tline) in window.iter().zip(target_lines.iter()) {
                if normalize(wline) == normalize(tline) {
                    matching += 1;
                }
            }

            let score = matching as f64 / target_line_count as f64;

            // Require at least 80% line match
            if score >= 0.8 {
                if best_match.is_none() || score > best_match.unwrap().2 {
                    best_match = Some((start, end, score));
                }
            }
        }

        if let Some((start, end, _score)) = best_match {
            // Return the exact substring from the original content
            let start_byte = content_lines[..start]
                .iter()
                .map(|l| l.len() + 1) // +1 for newline
                .sum::<usize>();
            let end_byte = content_lines[..end]
                .iter()
                .map(|l| l.len() + 1)
                .sum::<usize>();
            // Adjust for potential trailing newline
            let end_byte = end_byte.min(content.len());
            let slice = &content[start_byte..end_byte];
            // Trim trailing newline if target doesn't end with one
            if !target.ends_with('\n') && slice.ends_with('\n') {
                Some(&slice[..slice.len() - 1])
            } else {
                Some(slice)
            }
        } else {
            // Strategy 2: Single-line normalized match
            if !target.contains('\n') {
                for line in content.lines() {
                    if normalize(line) == target_normalized {
                        // Find this line's position in content
                        if let Some(pos) = content.find(line) {
                            return Some(&content[pos..pos + line.len()]);
                        }
                    }
                }
            }
            None
        }
    }

    /// Run post-edit validation if the file has a known extension.
    /// Returns a warning string if validation fails, None if ok or not applicable.
    async fn post_edit_validate(path: &std::path::Path) -> Option<String> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let check_cmd = match ext {
            "rs" => {
                // For Rust, check syntax with rustfmt --check (fast, no compilation)
                let dir = path.parent()?;
                // Find Cargo.toml by walking up
                let mut cargo_dir = dir.to_path_buf();
                loop {
                    if cargo_dir.join("Cargo.toml").exists() {
                        break;
                    }
                    if !cargo_dir.pop() {
                        return None; // No Cargo.toml found
                    }
                }
                Some(format!(
                    "cd {} && cargo check --message-format=short 2>&1 | head -20",
                    cargo_dir.display()
                ))
            }
            "py" => Some(format!("python3 -c \"import ast; ast.parse(open('{}').read())\" 2>&1", path.display())),
            "ts" | "tsx" => Some(format!("npx tsc --noEmit {} 2>&1 | head -10", path.display())),
            "js" | "jsx" => Some(format!("node --check {} 2>&1", path.display())),
            "json" => Some(format!("python3 -c \"import json; json.load(open('{}'))\" 2>&1", path.display())),
            "yaml" | "yml" => Some(format!("python3 -c \"import yaml; yaml.safe_load(open('{}'))\" 2>&1", path.display())),
            _ => None,
        };

        if let Some(cmd) = check_cmd {
            match tokio::process::Command::new("sh")
                .args(["-c", &cmd])
                .output()
                .await
            {
                Ok(output) => {
                    if !output.status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let msg = if stderr.is_empty() { stdout } else { stderr };
                        Some(format!("⚠️ Post-edit validation warning:\n{}", msg.trim()))
                    } else {
                        None
                    }
                }
                Err(_) => None, // Validation tool not available, skip
            }
        } else {
            None
        }
    }
}

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Edit a file by replacing text. Supports exact match with fuzzy fallback (whitespace-tolerant). Runs syntax validation after edit for supported languages (Rust, Python, TS, JS, JSON, YAML). Supports multiple edits in one call via 'edits' array."
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
                    "description": "Text to find and replace (exact match with fuzzy fallback)"
                },
                "new_string": {
                    "type": "string",
                    "description": "New text to replace with"
                },
                "edits": {
                    "type": "array",
                    "description": "Multiple edits to apply atomically: [{old_string, new_string}, ...]",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_string": { "type": "string" },
                            "new_string": { "type": "string" }
                        },
                        "required": ["old_string", "new_string"]
                    }
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path'"))?;

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

        // Build list of edits: either from 'edits' array or single old_string/new_string
        let edits: Vec<(String, String)> = if let Some(edits_arr) = input["edits"].as_array() {
            edits_arr
                .iter()
                .filter_map(|e| {
                    Some((
                        e["old_string"].as_str()?.to_string(),
                        e["new_string"].as_str()?.to_string(),
                    ))
                })
                .collect()
        } else {
            let old = input["old_string"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'old_string' or 'edits'"))?;
            let new = input["new_string"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'new_string'"))?;
            vec![(old.to_string(), new.to_string())]
        };

        if edits.is_empty() {
            return Ok(ToolResult {
                output: "No edits provided.".to_string(),
                is_error: true,
            });
        }

        // Apply all edits sequentially
        let mut current_content = content.clone();
        let mut results: Vec<String> = Vec::new();
        let mut any_fuzzy = false;

        for (i, (old_string, new_string)) in edits.iter().enumerate() {
            let edit_label = if edits.len() > 1 {
                format!("Edit {}/{}", i + 1, edits.len())
            } else {
                String::new()
            };

            let count = current_content.matches(old_string.as_str()).count();

            if count == 1 {
                // Exact match — apply directly
                current_content = current_content.replacen(old_string.as_str(), new_string.as_str(), 1);
                if !edit_label.is_empty() {
                    results.push(format!("{}: exact match ✓", edit_label));
                }
            } else if count > 1 {
                // Ambiguous — abort all edits (atomic: don't write partial changes)
                return Ok(ToolResult {
                    output: format!(
                        "{}old_string found {} times. Add more context to disambiguate. No edits applied.",
                        if edit_label.is_empty() { String::new() } else { format!("{}: ", edit_label) },
                        count
                    ),
                    is_error: true,
                });
            } else {
                // Exact match failed — try fuzzy
                if let Some(actual_match) = Self::fuzzy_find(&current_content, old_string) {
                    let actual_owned = actual_match.to_string();
                    current_content = current_content.replacen(&actual_owned, new_string.as_str(), 1);
                    any_fuzzy = true;
                    results.push(format!(
                        "{}fuzzy match (whitespace difference) ✓",
                        if edit_label.is_empty() { String::new() } else { format!("{}: ", edit_label) }
                    ));
                } else {
                    // No match at all — abort
                    return Ok(ToolResult {
                        output: format!(
                            "{}old_string not found (exact or fuzzy). No edits applied.\nTip: use read_file to check current content.",
                            if edit_label.is_empty() { String::new() } else { format!("{}: ", edit_label) }
                        ),
                        is_error: true,
                    });
                }
            }
        }

        // All edits applied — write file
        tokio::fs::write(&path, &current_content).await?;

        // Post-edit validation
        let validation = Self::post_edit_validate(&path).await;

        // Build output
        let mut output = format!("Edited {}", path.display());
        if edits.len() > 1 {
            output.push_str(&format!(" ({} edits applied)", edits.len()));
        }
        if any_fuzzy {
            output.push_str(" [fuzzy match used]");
        }
        if !results.is_empty() && edits.len() > 1 {
            output.push('\n');
            output.push_str(&results.join("\n"));
        }
        if let Some(warning) = validation {
            output.push('\n');
            output.push_str(&warning);
        }

        Ok(ToolResult {
            output,
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

// ─── Engram Trends Tool ──────────────────────────────────────

/// Show empathy trends per domain (observed user emotional patterns).
pub struct EngramTrendsTool {
    memory: Arc<MemoryManager>,
}

impl EngramTrendsTool {
    pub fn new(memory: Arc<MemoryManager>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for EngramTrendsTool {
    fn name(&self) -> &str {
        "engram_trends"
    }

    fn description(&self) -> &str {
        "Show emotional trends per domain. Tracks accumulated emotional valence (positive/negative) for different areas like coding, trading, research. Use this to understand which domains are going well or poorly."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _input: Value) -> anyhow::Result<ToolResult> {
        match self.memory.get_empathy_trends() {
            Ok(trends) => {
                if trends.is_empty() {
                    return Ok(ToolResult {
                        output: "No empathy trends recorded yet.".to_string(),
                        is_error: false,
                    });
                }

                let mut output = format!("Emotional trends ({} domains):\n\n", trends.len());
                for trend in &trends {
                    let sentiment = if trend.valence > 0.3 {
                        "😊 positive"
                    } else if trend.valence < -0.3 {
                        "😞 negative"
                    } else {
                        "😐 neutral"
                    };
                    let needs_attention = if trend.count >= 10 && trend.valence < -0.5 {
                        " ⚠️ needs attention"
                    } else {
                        ""
                    };
                    output.push_str(&format!(
                        "- **{}**: {} ({:.2} avg over {} events){}\n",
                        trend.domain, sentiment, trend.valence, trend.count, needs_attention
                    ));
                }

                Ok(ToolResult {
                    output,
                    is_error: false,
                })
            }
            Err(e) => Ok(ToolResult {
                output: format!("Failed to get empathy trends: {}", e),
                is_error: true,
            }),
        }
    }
}

// ─── Engram Behavior Stats Tool ──────────────────────────────

/// Show action success/failure rates.
pub struct EngramBehaviorStatsTool {
    memory: Arc<MemoryManager>,
}

impl EngramBehaviorStatsTool {
    pub fn new(memory: Arc<MemoryManager>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for EngramBehaviorStatsTool {
    fn name(&self) -> &str {
        "engram_behavior_stats"
    }

    fn description(&self) -> &str {
        "Show action/tool success and failure rates. Tracks which tools work well and which consistently fail. Use this to identify problematic patterns."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _input: Value) -> anyhow::Result<ToolResult> {
        match self.memory.get_behavior_stats() {
            Ok(stats) => {
                if stats.is_empty() {
                    return Ok(ToolResult {
                        output: "No behavior stats recorded yet.".to_string(),
                        is_error: false,
                    });
                }

                let mut output = format!("Behavior stats ({} actions):\n\n", stats.len());
                for stat in &stats {
                    let rating = if stat.score >= 0.8 {
                        "✅ excellent"
                    } else if stat.score >= 0.5 {
                        "⚠️ moderate"
                    } else if stat.score >= 0.2 {
                        "🔴 poor"
                    } else {
                        "❌ very poor"
                    };
                    let should_deprioritize = stat.total >= 10 && stat.score < 0.2;
                    let flag = if should_deprioritize { " 🚫 consider deprioritizing" } else { "" };
                    output.push_str(&format!(
                        "- **{}**: {} ({:.0}% success, {}/{} positive){}\n",
                        stat.action, rating, stat.score * 100.0, stat.positive, stat.total, flag
                    ));
                }

                // Also show deprioritized actions
                if let Ok(deprioritized) = self.memory.get_deprioritized_actions() {
                    if !deprioritized.is_empty() {
                        output.push_str(&format!(
                            "\n**Actions to deprioritize ({}):**\n",
                            deprioritized.len()
                        ));
                        for stat in &deprioritized {
                            output.push_str(&format!(
                                "- {} ({:.0}% success, {} attempts)\n",
                                stat.action, stat.score * 100.0, stat.total
                            ));
                        }
                    }
                }

                Ok(ToolResult {
                    output,
                    is_error: false,
                })
            }
            Err(e) => Ok(ToolResult {
                output: format!("Failed to get behavior stats: {}", e),
                is_error: true,
            }),
        }
    }
}

// ─── Engram Soul Suggestions Tool ────────────────────────────

/// Get SOUL.md update suggestions based on empathy patterns.
pub struct EngramSoulSuggestionsTool {
    memory: Arc<MemoryManager>,
}

impl EngramSoulSuggestionsTool {
    pub fn new(memory: Arc<MemoryManager>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for EngramSoulSuggestionsTool {
    fn name(&self) -> &str {
        "engram_soul_suggestions"
    }

    fn description(&self) -> &str {
        "Get SOUL.md update suggestions based on accumulated emotional patterns. When domains show persistent negative trends, suggests adding drives or notes to address them."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _input: Value) -> anyhow::Result<ToolResult> {
        match self.memory.suggest_soul_updates() {
            Ok(suggestions) => {
                if suggestions.is_empty() {
                    return Ok(ToolResult {
                        output: "No SOUL.md update suggestions at this time. Emotional trends are within normal parameters.".to_string(),
                        is_error: false,
                    });
                }

                let mut output = format!("SOUL.md update suggestions ({}):\n\n", suggestions.len());
                for (i, suggestion) in suggestions.iter().enumerate() {
                    output.push_str(&format!(
                        "{}. **[{}] {}**\n   {}\n   (Based on: {} domain, {:.2} valence over {} events)\n\n",
                        i + 1,
                        suggestion.action,
                        suggestion.domain,
                        suggestion.content,
                        suggestion.trend.domain,
                        suggestion.trend.valence,
                        suggestion.trend.count
                    ));
                }

                // Also check heartbeat suggestions
                if let Ok(heartbeat_suggestions) = self.memory.suggest_heartbeat_updates() {
                    if !heartbeat_suggestions.is_empty() {
                        output.push_str(&format!(
                            "\n**HEARTBEAT.md suggestions ({}):**\n",
                            heartbeat_suggestions.len()
                        ));
                        for suggestion in &heartbeat_suggestions {
                            output.push_str(&format!(
                                "- {} '{}' ({:.0}% success rate, {} attempts)\n",
                                suggestion.suggestion,
                                suggestion.action,
                                suggestion.stats.score * 100.0,
                                suggestion.stats.total
                            ));
                        }
                    }
                }

                Ok(ToolResult {
                    output,
                    is_error: false,
                })
            }
            Err(e) => Ok(ToolResult {
                output: format!("Failed to get soul suggestions: {}", e),
                is_error: true,
            }),
        }
    }
}

// ─── Knowledge Compiler Tools ────────────────────────────────

use engramai::compiler::{
    api::{MaintenanceApi, QueryOpts},
    compilation::{MemorySnapshot, simple_hash_embedding},
    discovery::TopicDiscovery,
    storage::SqliteKnowledgeStore,
    llm::{NoopProvider, LlmProvider as KcLlmProvider},
    types::{LlmRequest as KcLlmRequest, LlmResponse as KcLlmResponse, LlmError as KcLlmError, ProviderMetadata as KcProviderMetadata, TokenUsage as KcTokenUsage},
    KcConfig,
    KnowledgeStore as _,
};

/// LLM provider for Knowledge Compiler that wraps RustClaw's LlmClient.
/// Bridges async LlmClient → sync KcLlmProvider via block_in_place.
pub struct RustClawKcProvider {
    pub client: Box<dyn crate::llm::LlmClient>,
    pub runtime: tokio::runtime::Handle,
    pub model: String,
}

impl KcLlmProvider for RustClawKcProvider {
    fn complete(&self, request: &KcLlmRequest) -> Result<KcLlmResponse, KcLlmError> {
        let system = match &request.task {
            engramai::compiler::types::LlmTask::GenerateTitle =>
                "You are a title generator. Produce a short, descriptive title for the provided content. Respond with ONLY the title, nothing else.",
            engramai::compiler::types::LlmTask::Compile =>
                "You are a knowledge compiler. Synthesize the provided memories into a coherent topic page.",
            engramai::compiler::types::LlmTask::Summarize =>
                "You are a summarizer. Produce a concise summary of the provided content.",
            _ => "You are a helpful assistant.",
        };

        let messages = vec![crate::llm::Message::text("user", &request.prompt)];

        let start = std::time::Instant::now();
        let result = tokio::task::block_in_place(|| {
            self.runtime.block_on(
                self.client.chat_with_model(system, &messages, &[], &self.model)
            )
        });
        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(resp) => {
                let content = resp.text.unwrap_or_default();
                Ok(KcLlmResponse {
                    content,
                    usage: KcTokenUsage {
                        input_tokens: resp.usage.input_tokens,
                        output_tokens: resp.usage.output_tokens,
                    },
                    model: self.model.clone(),
                    duration_ms,
                })
            }
            Err(e) => Err(KcLlmError::ProviderUnavailable(e.to_string())),
        }
    }

    fn metadata(&self) -> KcProviderMetadata {
        KcProviderMetadata {
            name: "rustclaw".to_string(),
            model: self.model.clone(),
            max_context_tokens: 200000,
            supports_streaming: false,
        }
    }

    fn health_check(&self) -> Result<(), KcLlmError> {
        Ok(()) // RustClaw client handles auth internally
    }
}

/// Query compiled knowledge topics.
pub struct KnowledgeQueryTool {
    memory: Arc<MemoryManager>,
}

impl KnowledgeQueryTool {
    pub fn new(memory: Arc<MemoryManager>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for KnowledgeQueryTool {
    fn name(&self) -> &str {
        "knowledge_query"
    }

    fn description(&self) -> &str {
        "Search compiled knowledge topics. Knowledge Compiler aggregates related memories into structured topic pages for efficient retrieval."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query to find relevant knowledge topics"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of topics to return (default: 10)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let query = input["query"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' parameter"))?;
        let limit = input["limit"].as_u64().unwrap_or(10) as usize;

        let db_path = self.memory.db_path().to_string();
        let store = SqliteKnowledgeStore::open(std::path::Path::new(&db_path))
            .map_err(|e| anyhow::anyhow!("Failed to open KC store: {}", e))?;
        store.init_schema()
            .map_err(|e| anyhow::anyhow!("Failed to init KC schema: {}", e))?;

        let config = KcConfig::load();
        let api = MaintenanceApi::new(store, config);
        let opts = QueryOpts { limit, include_archived: false };

        match api.query(query, &opts) {
            Ok(results) => {
                if results.is_empty() {
                    return Ok(ToolResult {
                        output: format!("No knowledge topics matching '{}'.", query),
                        is_error: false,
                    });
                }

                let mut output = format!("Found {} knowledge topics:\n\n", results.len());
                for r in &results {
                    output.push_str(&format!(
                        "- [{}] {} (status: {:?}, relevance: {:.1})\n",
                        r.topic_id, r.title, r.status, r.relevance
                    ));
                    if !r.summary.is_empty() {
                        let preview: String = r.summary.chars().take(120).collect();
                        output.push_str(&format!("  {}\n", preview));
                    }
                }
                Ok(ToolResult { output, is_error: false })
            }
            Err(e) => Ok(ToolResult {
                output: format!("Knowledge query failed: {}", e),
                is_error: true,
            }),
        }
    }
}

/// List all compiled knowledge topics.
pub struct KnowledgeListTool {
    memory: Arc<MemoryManager>,
}

impl KnowledgeListTool {
    pub fn new(memory: Arc<MemoryManager>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for KnowledgeListTool {
    fn name(&self) -> &str {
        "knowledge_list"
    }

    fn description(&self) -> &str {
        "List all compiled knowledge topics with their status and quality scores."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of topics to list (default: 50)"
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let limit = input["limit"].as_u64().unwrap_or(50) as usize;

        let db_path = self.memory.db_path().to_string();
        let store = SqliteKnowledgeStore::open(std::path::Path::new(&db_path))
            .map_err(|e| anyhow::anyhow!("Failed to open KC store: {}", e))?;
        store.init_schema()
            .map_err(|e| anyhow::anyhow!("Failed to init KC schema: {}", e))?;

        let config = KcConfig::load();
        let api = MaintenanceApi::new(store, config);

        match api.list() {
            Ok(pages) => {
                let total = pages.len();
                let shown: Vec<_> = pages.into_iter().take(limit).collect();

                let mut output = format!("Knowledge Base: {} topics total\n\n", total);
                for page in &shown {
                    let quality = page.metadata.quality_score
                        .map(|s| format!(" q={:.2}", s))
                        .unwrap_or_default();
                    let sources = page.metadata.source_memory_ids.len();
                    output.push_str(&format!(
                        "- [{}] {} (v{}, {:?}, {} sources{})\n",
                        page.id.0, page.title, page.version, page.status, sources, quality
                    ));
                }
                if total > limit {
                    output.push_str(&format!("\n... and {} more topics", total - limit));
                }
                Ok(ToolResult { output, is_error: false })
            }
            Err(e) => Ok(ToolResult {
                output: format!("Failed to list knowledge topics: {}", e),
                is_error: true,
            }),
        }
    }
}

/// Get knowledge base health report.
pub struct KnowledgeHealthTool {
    memory: Arc<MemoryManager>,
}

impl KnowledgeHealthTool {
    pub fn new(memory: Arc<MemoryManager>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for KnowledgeHealthTool {
    fn name(&self) -> &str {
        "knowledge_health"
    }

    fn description(&self) -> &str {
        "Get a health report for the knowledge base: stale topics, conflicts, decay status, and quality metrics."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _input: Value) -> anyhow::Result<ToolResult> {
        let db_path = self.memory.db_path().to_string();
        let store = SqliteKnowledgeStore::open(std::path::Path::new(&db_path))
            .map_err(|e| anyhow::anyhow!("Failed to open KC store: {}", e))?;
        store.init_schema()
            .map_err(|e| anyhow::anyhow!("Failed to init KC schema: {}", e))?;

        let config = KcConfig::load();
        let api = MaintenanceApi::new(store, config);

        match api.health_report() {
            Ok(report) => {
                let mut output = format!(
                    "Knowledge Base Health Report:\n\
                     \n  Total topics: {}\
                     \n  Stale topics: {}\
                     \n  Conflicts: {}\
                     \n  Broken links: {}\n",
                    report.total_topics,
                    report.stale_topics.len(),
                    report.conflicts.len(),
                    report.broken_links.len(),
                );
                if !report.recommendations.is_empty() {
                    output.push_str(&format!("\n  Recommendations ({}):\n", report.recommendations.len()));
                    for rec in &report.recommendations {
                        output.push_str(&format!("    - [P{}] {} — {}\n", rec.priority, rec.action, rec.reason));
                    }
                }
                Ok(ToolResult { output, is_error: false })
            }
            Err(e) => Ok(ToolResult {
                output: format!("Health report failed: {}", e),
                is_error: true,
            }),
        }
    }
}

/// Compile memories into knowledge topics.
pub struct KnowledgeCompileTool {
    memory: Arc<MemoryManager>,
    llm_client: Arc<tokio::sync::RwLock<Box<dyn crate::llm::LlmClient>>>,
}

impl KnowledgeCompileTool {
    pub fn new(memory: Arc<MemoryManager>, llm_client: Arc<tokio::sync::RwLock<Box<dyn crate::llm::LlmClient>>>) -> Self {
        Self { memory, llm_client }
    }
}

#[async_trait]
impl Tool for KnowledgeCompileTool {
    fn name(&self) -> &str {
        "knowledge_compile"
    }

    fn description(&self) -> &str {
        "Compile raw memories into structured knowledge topics. Discovers clusters of related memories and synthesizes them into topic pages. Use dry_run=true to preview without changes."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "dry_run": {
                    "type": "boolean",
                    "description": "Preview what would be compiled without making changes (default: false)"
                },
                "topic": {
                    "type": "string",
                    "description": "Recompile a specific topic by ID (optional — if omitted, runs auto-discovery)"
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let dry_run = input["dry_run"].as_bool().unwrap_or(false);
        let _topic_id = input["topic"].as_str().map(String::from);

        let db_path = self.memory.db_path().to_string();

        // Load all memories (scope to drop the !Send MutexGuard before any await)
        let all_memories = {
            let engram = self.memory.lock_engram()?;
            engram.list_ns(None, None)
                .map_err(|e| anyhow::anyhow!("Failed to list memories: {}", e))?
        };

        if all_memories.is_empty() {
            return Ok(ToolResult {
                output: "No memories found. Nothing to compile.".to_string(),
                is_error: false,
            });
        }

        // Load embeddings
        let storage = engramai::storage::Storage::new(&db_path)
            .map_err(|e| anyhow::anyhow!("Failed to open storage: {}", e))?;
        let emb_config = engramai::embeddings::EmbeddingConfig::default();
        let model_id = emb_config.model_id();
        let embedding_map: std::collections::HashMap<String, Vec<f32>> = storage
            .get_all_embeddings(&model_id)
            .unwrap_or_default()
            .into_iter()
            .collect();

        // Convert to snapshots
        let snapshots: Vec<MemorySnapshot> = all_memories.iter().map(|m| {
            let tags = m.metadata.as_ref()
                .and_then(|v| v.get("tags"))
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            MemorySnapshot {
                id: m.id.clone(),
                content: m.content.clone(),
                memory_type: format!("{:?}", m.memory_type).to_lowercase(),
                importance: m.importance,
                created_at: m.created_at,
                updated_at: m.created_at,
                tags,
                embedding: embedding_map.get(&m.id).cloned(),
                dimensions: None,
                type_weights: None,
                confidence: None,
                valence: None,
            }
        }).collect();

        let store = SqliteKnowledgeStore::open(std::path::Path::new(&db_path))
            .map_err(|e| anyhow::anyhow!("Failed to open KC store: {}", e))?;
        store.init_schema()
            .map_err(|e| anyhow::anyhow!("Failed to init KC schema: {}", e))?;

        let config = KcConfig::load();
        let api = MaintenanceApi::new(store, config);

        if dry_run {
            // Use discovery to preview clusters
            let discovery = TopicDiscovery::new(3);
            let embeddings: Vec<(String, Vec<f32>)> = snapshots.iter()
                .map(|m| {
                    let emb = m.embedding.clone()
                        .unwrap_or_else(|| simple_hash_embedding(&m.content, 64));
                    (m.id.clone(), emb)
                })
                .collect();
            let candidates = discovery.discover(&embeddings);

            let mut output = format!(
                "Dry run: {} memories → {} potential topics\n\n",
                snapshots.len(), candidates.len()
            );
            for (i, c) in candidates.iter().enumerate().take(20) {
                let title = c.suggested_title.as_deref().unwrap_or("(untitled)");
                output.push_str(&format!("  {}. {} ({} memories)\n", i + 1, title, c.memories.len()));
            }
            if candidates.len() > 20 {
                output.push_str(&format!("  ... and {} more candidates\n", candidates.len() - 20));
            }
            Ok(ToolResult { output, is_error: false })
        } else {
            // Create LLM provider for topic labeling — reuses RustClaw's LlmClient.
            // Use `.read().await` (async context); the old `blocking_read()` panicked
            // inside the tokio runtime due to deadlock protection.
            let kc_provider: Option<Box<dyn KcLlmProvider>> = {
                let guard = self.llm_client.read().await;
                Some(Box::new(RustClawKcProvider {
                    client: guard.clone_boxed(),
                    runtime: tokio::runtime::Handle::current(),
                    model: "claude-haiku-4-5-20251001".to_string(),
                }) as Box<dyn KcLlmProvider>)
            };

            // Actually compile — use recompile_all to purge old topics first
            let result = match &kc_provider {
                Some(provider) => api.recompile_all_dyn(Some(provider.as_ref()), &snapshots),
                None => api.recompile_all::<NoopProvider>(None, &snapshots),
            };
            match result {
                Ok((purged, pages)) => {
                    let mut output = if purged > 0 {
                        format!("🔄 Purged {} old topics, compiled {} new topics:\n\n", purged, pages.len())
                    } else {
                        format!("✅ Compiled {} new topics:\n\n", pages.len())
                    };
                    for p in &pages {
                        let quality = p.metadata.quality_score
                            .map(|s| format!(" q={:.2}", s))
                            .unwrap_or_default();
                        output.push_str(&format!(
                            "  [{}] {} (v{}{})\n",
                            p.id.0, p.title, p.version, quality
                        ));
                    }
                    Ok(ToolResult { output, is_error: false })
                }
                Err(e) => Ok(ToolResult {
                    output: format!("Compilation failed: {}", e),
                    is_error: true,
                }),
            }
        }
    }
}

// ─── TTS (Text-to-Speech) Tool ───────────────────────────────

/// Text-to-Speech tool using edge-tts.
pub struct TtsTool;

#[async_trait]
impl Tool for TtsTool {
    fn name(&self) -> &str {
        "tts"
    }

    fn description(&self) -> &str {
        "Convert text to speech (generates OGG audio file). For generating audio files on demand. Do NOT use for voice mode replies — voice mode is handled automatically by the framework via set_voice_mode tool."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Text to convert to speech"
                },
                "voice": {
                    "type": "string",
                    "description": "Voice name (default: en-US-EmmaMultilingualNeural). Other options: zh-CN-YunyangNeural, zh-CN-XiaoxiaoNeural, en-GB-SoniaNeural"
                }
            },
            "required": ["text"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let text = input["text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'text' parameter"))?;
        let voice = input["voice"]
            .as_str()
            .unwrap_or("en-US-EmmaMultilingualNeural");

        // Use the TTS module
        let config = crate::tts::TtsConfig {
            voice: voice.to_string(),
            ..Default::default()
        };

        match crate::tts::synthesize(text, &config).await {
            Ok(path) => Ok(ToolResult {
                output: format!("Audio generated: {}", path),
                is_error: false,
            }),
            Err(e) => Ok(ToolResult {
                output: format!("TTS failed: {}", e),
                is_error: true,
            }),
        }
    }
}

// ─── STT (Speech-to-Text) Tool ───────────────────────────────

/// Speech-to-Text tool using Whisper.
pub struct SttTool;

#[async_trait]
impl Tool for SttTool {
    fn name(&self) -> &str {
        "stt"
    }

    fn description(&self) -> &str {
        "Transcribe audio to text using Whisper. Supports OGG, WAV, MP3, and other common audio formats. Returns the transcribed text."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the audio file to transcribe"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let path = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;

        match crate::stt::transcribe(path).await {
            Ok(text) => Ok(ToolResult {
                output: text,
                is_error: false,
            }),
            Err(e) => Ok(ToolResult {
                output: format!("STT failed: {}", e),
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
        let wait = true; // Always wait — fire-and-forget disabled (produces blind spots)

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
    notify_slot: Arc<std::sync::Mutex<Option<crate::ritual_runner::NotifyFn>>>,
    session_key_slot: Arc<std::sync::Mutex<Option<String>>>,
    event_tx: Option<tokio::sync::broadcast::Sender<crate::events::SubAgentEvent>>,
}

impl SpawnSpecialistTool {
    pub fn new(
        runner: SharedAgentRunner,
        orchestrator: Option<SharedOrchestrator>,
        notify_slot: Arc<std::sync::Mutex<Option<crate::ritual_runner::NotifyFn>>>,
        session_key_slot: Arc<std::sync::Mutex<Option<String>>>,
        event_tx: Option<tokio::sync::broadcast::Sender<crate::events::SubAgentEvent>>,
    ) -> Self {
        Self { runner, orchestrator, notify_slot, session_key_slot, event_tx }
    }

    /// Fire-and-forget notification via the shared notify slot (Telegram).
    fn fire_notify(&self, message: String) {
        let slot = self.notify_slot.lock().unwrap();
        if let Some(notify) = slot.as_ref() {
            let notify = notify.clone();
            tokio::spawn(async move {
                notify(message).await;
            });
        }
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
                    "description": "Whether to wait for completion (default: true). Set to false for fire-and-forget.",
                    "deprecated": true
                },
                "skill": {
                    "type": "string",
                    "description": "Skill name to inject (e.g., 'review-requirements', 'review-design'). The skill's SKILL.md content is prepended to the task prompt, giving the sub-agent full instructions."
                },
                "files": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "File paths to pre-load into the sub-agent's context. Content is included directly so the sub-agent doesn't waste iterations reading these files. Paths relative to workspace."
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
        let raw_task = input["task"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'task' parameter"))?;

        let skill_name = input["skill"].as_str();
        let role = input["role"].as_str();
        let model_override = input["model"].as_str();
        let workspace_override = input["workspace"].as_str();

        // Parse files to pre-load
        let preload_files: Vec<String> = input["files"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        // Resolve skill: explicit param → auto-detect via SkillRegistry
        let skill_registry = &runner.workspace().skill_registry;
        let effective_skill: Option<String> = skill_name.map(String::from).or_else(|| {
            let task_lower = raw_task.to_lowercase();
            for skill in skill_registry.all() {
                let name: &str = skill.name();
                let name_spaced = name.replace('-', " ");
                if task_lower.contains(&name_spaced) || task_lower.contains(name) {
                    tracing::info!("Auto-detected skill '{}' from task content via SkillRegistry", name);
                    return Some(name.to_string());
                }
            }
            None
        });

        // Skill injection is now handled by run_subagent() via SubAgentOptions.skill
        // We just pass the raw task + skill name through.
        let task: String = raw_task.to_string();
        let max_iterations = input["max_iterations"].as_u64().unwrap_or(0) as u32; // 0 = use AgentType default

        // Pre-load files into context blocks
        let workspace_root = workspace_override
            .or(runner.config().workspace.as_deref())
            .unwrap_or(".");
        let workspace_path = std::path::Path::new(workspace_root);
        let preload_context: Vec<crate::agent::ContextBlock> = if !preload_files.is_empty() {
            let full_paths: Vec<std::path::PathBuf> = preload_files.iter()
                .map(|rel| {
                    let p = std::path::Path::new(rel);
                    if p.is_absolute() { p.to_path_buf() } else { workspace_path.join(rel) }
                })
                .filter(|p| p.exists())
                .collect();
            if full_paths.len() < preload_files.len() {
                let missing: Vec<&str> = preload_files.iter()
                    .filter(|rel| {
                        let p = std::path::Path::new(rel.as_str());
                        let full = if p.is_absolute() { p.to_path_buf() } else { workspace_path.join(rel) };
                        !full.exists()
                    })
                    .map(|s| s.as_str())
                    .collect();
                tracing::warn!("Pre-load: {} files not found: {:?}", missing.len(), missing);
            }
            crate::ritual_runner::preload_files_with_budget(&full_paths, workspace_path, 120_000)
        } else {
            vec![]
        };
        let wait = true; // Always wait — fire-and-forget disabled (produces blind spots)

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
            // Completion/failure will be notified via Telegram + injected back to parent session
            let runner_clone = runner.clone();
            let task_owned = task.to_string();
            let final_config_clone = final_config.clone();
            let notify_slot = self.notify_slot.clone();
            let notify_slot_ping = self.notify_slot.clone();
            // Capture parent session key for completion injection
            let parent_session_key = self.session_key_slot.lock().unwrap().clone();
            let event_tx = self.event_tx.clone();
            let start_time = std::time::Instant::now();
            let task_summary = if task.chars().count() > 80 {
                format!("{}...", task.chars().take(80).collect::<String>())
            } else {
                task.to_string()
            };
            
            tokio::spawn(async move {
                // Helper to send notification
                let send_notify = |msg: String| {
                    let slot = notify_slot.lock().unwrap();
                    if let Some(notify) = slot.as_ref() {
                        let notify = notify.clone();
                        tokio::spawn(async move { notify(msg).await; });
                    }
                };

                // Map role to agent type
                let agent_type = match final_config_clone.name.as_deref() {
                    Some(r) if r.contains("explorer") || r.contains("researcher") || r.contains("Explorer") => &crate::agent::AgentType::EXPLORER,
                    Some(r) if r.contains("reviewer") || r.contains("Reviewer") => &crate::agent::AgentType::REVIEWER,
                    Some(r) if r.contains("planner") || r.contains("architect") || r.contains("Planner") => &crate::agent::AgentType::PLANNER,
                    _ => &crate::agent::AgentType::CODER,
                };
                // Spawn a periodic progress ping (every 5 minutes)
                    let ping_notify_slot = notify_slot_ping.clone();
                    let ping_task_summary = task_summary.clone();
                    let ping_agent_id = final_config_clone.id.clone();
                    let ping_start = start_time;
                    let ping_handle = tokio::spawn(async move {
                        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
                        interval.tick().await; // skip first immediate tick
                        loop {
                            interval.tick().await;
                            let elapsed = format_subagent_duration(ping_start.elapsed());
                            let msg = format!(
                                "⏳ **Sub-agent still running** ({})\n📋 Task: {}\n🆔 ID: {}",
                                elapsed, ping_task_summary, ping_agent_id
                            );
                            let notify_opt = {
                                let slot = ping_notify_slot.lock().unwrap();
                                slot.as_ref().cloned()
                            };
                            if let Some(notify) = notify_opt {
                                notify(msg).await;
                            }
                        }
                    });

                    let options = crate::agent::SubAgentOptions {
                        model: final_config_clone.model.clone(),
                        max_iterations: if effective_max_iterations > 0 { Some(effective_max_iterations) } else { None },
                        workspace: final_config_clone.workspace.as_ref().map(std::path::PathBuf::from),
                        context: preload_context.clone(),
                        skill: effective_skill.clone(),
                    };
                    let sub_result = runner_clone.run_subagent(agent_type, &task_owned, options).await;
                    ping_handle.abort(); // Stop progress pings

                    let elapsed = start_time.elapsed();
                    let duration = format_subagent_duration(elapsed);

                    if sub_result.outcome.is_success() {
                        let result_preview = if sub_result.output.chars().count() > 200 {
                            format!("{}...", sub_result.output.chars().take(200).collect::<String>())
                        } else {
                            sub_result.output.clone()
                        };
                        tracing::info!("Background sub-agent {} completed: {} chars, {} tokens, {} files",
                            sub_result.agent_id, sub_result.output.len(), sub_result.tokens, sub_result.files_modified.len());
                        let completion_msg = format!(
                            "✅ **Sub-agent completed** ({})\n\n\
                             📋 Task: {}\n\
                             🆔 ID: {}\n\
                             📝 Result:\n{}\n\
                             📁 Files: {:?}",
                            duration, task_summary, sub_result.agent_id, result_preview, sub_result.files_modified
                        );
                        send_notify(completion_msg.clone());
                        if let Some(ref tx) = event_tx {
                            let _ = tx.send(crate::events::SubAgentEvent::Completed {
                                task_id: sub_result.agent_id.clone(),
                                parent_session_key: parent_session_key.clone().unwrap_or_default(),
                                task_summary: task_summary.clone(),
                                result_preview: result_preview.clone(),
                                files_modified: sub_result.files_modified.clone(),
                                duration_secs: elapsed.as_secs_f64(),
                            });
                        }
                    } else {
                        let error_display = sub_result.outcome.display();
                        tracing::error!("Background sub-agent {} failed: {}", sub_result.agent_id, error_display);
                        let fail_msg = format!(
                            "❌ **Sub-agent failed** ({})\n\n\
                             📋 Task: {}\n\
                             🆔 ID: {}\n\
                             💥 {}\n\
                             🪙 Tokens: {}, Turns: {}",
                            duration, task_summary, sub_result.agent_id, error_display, sub_result.tokens, sub_result.turns
                        );
                        send_notify(fail_msg.clone());
                        if let Some(ref tx) = event_tx {
                            let _ = tx.send(crate::events::SubAgentEvent::Failed {
                                task_id: sub_result.agent_id.clone(),
                                parent_session_key: parent_session_key.clone().unwrap_or_default(),
                                task_summary: task_summary.clone(),
                                error: error_display.clone(),
                                files_modified: sub_result.files_modified.clone(),
                                duration_secs: elapsed.as_secs_f64(),
                            });
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

        // Wait mode: run typed sub-agent and wait for result
        let agent_type = match role {
            Some("explorer" | "researcher") => &crate::agent::AgentType::EXPLORER,
            Some("reviewer") => &crate::agent::AgentType::REVIEWER,
            Some("planner" | "architect") => &crate::agent::AgentType::PLANNER,
            _ => &crate::agent::AgentType::CODER,
        };
        let options = crate::agent::SubAgentOptions {
            model: final_config.model.clone(),
            max_iterations: if effective_max_iterations > 0 { Some(effective_max_iterations) } else { None },
            workspace: final_config.workspace.as_ref().map(std::path::PathBuf::from),
            context: preload_context,
            skill: effective_skill,
        };
        let sub_result = runner.run_subagent(agent_type, &task, options).await;
        tracing::info!("Sub-agent {} — {} ({} tokens, {} turns, {} files)",
            sub_result.agent_id, sub_result.outcome.display(), sub_result.tokens, sub_result.turns, sub_result.files_modified.len());

        if sub_result.outcome.is_success() {
            let max_output = 8000;
            let truncated_result = if sub_result.output.chars().count() > max_output {
                let preview: String = sub_result.output.chars().take(max_output).collect();
                format!("{}\n\n... (truncated from {} chars — sub-agent wrote full output to files)", preview, sub_result.output.chars().count())
            } else {
                sub_result.output
            };
            Ok(ToolResult {
                output: format!(
                    "## Sub-agent '{}' completed\n\n### Result:\n{}\n\n### Files modified:\n{}",
                    sub_result.agent_id, truncated_result,
                    if sub_result.files_modified.is_empty() { "(none)".to_string() }
                    else { sub_result.files_modified.join("\n") }
                ),
                is_error: false,
            })
        } else {
            Ok(ToolResult {
                output: format!("Sub-agent '{}' failed: {}\nTokens: {}, Turns: {}\nFiles modified before failure: {:?}",
                    sub_result.agent_id, sub_result.outcome.display(), sub_result.tokens, sub_result.turns, sub_result.files_modified),
                is_error: true,
            })
        }
    }
}

// ─── GID Tools ───────────────────────────────────────────────

type SharedGraph = Arc<tokio::sync::RwLock<Graph>>;
type SharedPath = Arc<String>;

/// Manages workspace graph and lazily-loaded external project graphs.
/// All GID tools receive an `Arc<GraphManager>` instead of direct `(SharedGraph, SharedPath)`.
/// When a tool's input includes a `"project"` parameter (path to a project directory),
/// it resolves to that project's graph; otherwise it uses the workspace default.
struct GraphManager {
    /// Workspace graph (default).
    workspace_graph: SharedGraph,
    /// Path to workspace graph.yml (e.g. ".gid/graph.yml").
    workspace_path: SharedPath,
    /// Path to workspace .gid/ directory.
    workspace_gid_dir: PathBuf,
    /// Cache of loaded external project graphs: project_root → (graph, path).
    cache: tokio::sync::RwLock<std::collections::HashMap<PathBuf, (SharedGraph, SharedPath)>>,
}

impl GraphManager {
    fn new(graph_path: &str) -> Self {
        let path = std::path::Path::new(graph_path);
        let gid_dir = path
            .parent()
            .unwrap_or(std::path::Path::new(".gid"))
            .to_path_buf();
        let backend = detect_backend(&gid_dir);
        let (graph, effective_path) = match backend {
            StorageBackend::Sqlite => {
                let g = load_graph_auto(&gid_dir, Some(StorageBackend::Sqlite))
                    .unwrap_or_default();
                let p = gid_dir.join("graph.db").to_string_lossy().to_string();
                (g, p)
            }
            _ => {
                let g = gid_load_graph(path).unwrap_or_default();
                (g, graph_path.to_string())
            }
        };
        let graph = Arc::new(tokio::sync::RwLock::new(graph));
        Self {
            workspace_graph: graph,
            workspace_path: Arc::new(effective_path),
            workspace_gid_dir: gid_dir,
            cache: tokio::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// Resolve a (graph, path) pair from tool input.
    /// If input contains `"project"`: load/cache that project's graph.
    /// Otherwise: return the workspace graph.
    async fn resolve(&self, input: &Value) -> anyhow::Result<(SharedGraph, SharedPath)> {
        match input.get("project").and_then(|v| v.as_str()) {
            None => Ok((self.workspace_graph.clone(), self.workspace_path.clone())),
            Some(project_dir) => self.resolve_external(project_dir).await,
        }
    }

    /// Resolve the .gid/ directory path from tool input.
    async fn resolve_gid_dir(&self, input: &Value) -> anyhow::Result<PathBuf> {
        match input.get("project").and_then(|v| v.as_str()) {
            None => Ok(self.workspace_gid_dir.clone()),
            Some(project_dir) => {
                let project_path = std::path::Path::new(project_dir);
                let root = find_project_root(project_path)
                    .unwrap_or_else(|| project_path.to_path_buf());
                Ok(root.join(".gid"))
            }
        }
    }

    /// Load (or return cached) an external project's graph.
    async fn resolve_external(&self, project_dir: &str) -> anyhow::Result<(SharedGraph, SharedPath)> {
        let project_path = std::path::Path::new(project_dir);
        let root = find_project_root(project_path)
            .unwrap_or_else(|| project_path.to_path_buf());
        let canonical = root.canonicalize().unwrap_or_else(|_| root.clone());

        // Check cache first.
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(&canonical) {
                return Ok(entry.clone());
            }
        }

        // Load from disk.
        let gid_dir = root.join(".gid");
        std::fs::create_dir_all(&gid_dir)?;
        let backend = detect_backend(&gid_dir);
        let graph_file = match backend {
            StorageBackend::Sqlite => gid_dir.join("graph.db"),
            _ => gid_dir.join("graph.yml"),
        };
        let graph = load_graph_auto(&gid_dir, Some(backend)).unwrap_or_default();
        let shared_graph = Arc::new(tokio::sync::RwLock::new(graph));
        let shared_path = Arc::new(graph_file.to_string_lossy().to_string());

        // Cache it.
        let entry = (shared_graph.clone(), shared_path.clone());
        {
            let mut cache = self.cache.write().await;
            cache.insert(canonical, entry);
        }

        Ok((shared_graph, shared_path))
    }
}

/// Helper: save graph to disk after mutation.
/// Format a sub-agent duration in human-readable form.
fn format_subagent_duration(elapsed: std::time::Duration) -> String {
    let secs = elapsed.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    }
}

fn save_gid_graph(graph: &Graph, path: &str) -> anyhow::Result<()> {
    let path = std::path::Path::new(path);
    if path.extension().and_then(|e| e.to_str()) == Some("db") {
        save_graph_auto(graph, path.parent().unwrap_or(std::path::Path::new(".")), Some(StorageBackend::Sqlite))
            .map_err(|e| anyhow::anyhow!("{}", e))
    } else {
        gid_save_graph(graph, path)
    }
}

/// Common JSON Schema property for cross-project access.
/// Add this to any GID tool's `input_schema` properties to enable the `project` parameter.
fn gid_project_property() -> (String, Value) {
    ("project".to_string(), serde_json::json!({
        "type": "string",
        "description": "Path to an external project directory. If provided, operates on that project's graph instead of the workspace graph."
    }))
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
    mgr: Arc<GraphManager>,
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
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "description": "Filter by status: todo, in_progress, done, blocked, cancelled",
                    "enum": ["todo", "in_progress", "done", "blocked", "cancelled"]
                },
                "node_type": {
                    "type": "string",
                    "description": "Filter by node type. Default shows project nodes only (task/feature/component/legacy). Use 'code' for code nodes, 'all' for everything."
                },
                proj_key: proj_val,
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let (graph_arc, _path) = self.mgr.resolve(&input).await?;
        let graph = graph_arc.read().await;
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

        // Apply node_type filter (default: project nodes only)
        let node_type_filter = input["node_type"].as_str();
        let nodes: Vec<&Node> = nodes.into_iter().filter(|n| {
            match node_type_filter {
                Some("all") => true,
                Some("code") => n.source.as_deref() == Some("extract"),
                Some(t) => n.node_type.as_deref() == Some(t),
                None => {
                    // Default: project nodes only (source != "extract")
                    n.source.as_deref() != Some("extract")
                }
            }
        }).collect();

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
    mgr: Arc<GraphManager>,
}

#[async_trait]
impl Tool for GidAddTaskTool {
    fn name(&self) -> &str {
        "gid_add_task"
    }

    fn description(&self) -> &str {
        "Add a node to the project graph (task, feature, component, or planned code node)."
    }

    fn input_schema(&self) -> Value {
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Unique task ID (e.g. 'impl-auth')" },
                "title": { "type": "string", "description": "Task title" },
                "description": { "type": "string", "description": "Detailed description (optional)" },
                "status": { "type": "string", "enum": ["todo", "in_progress", "done", "blocked"], "description": "Initial status (default: todo)" },
                "priority": { "type": "integer", "description": "Priority 0-255 (0=highest, optional)" },
                "tags": { "type": "array", "items": { "type": "string" }, "description": "Tags (optional)" },
                "depends_on": { "type": "array", "items": { "type": "string" }, "description": "Task IDs this depends on (optional)" },
                "node_type": { "type": "string", "description": "Node type: task, feature, component, code, etc. (optional)" },
                "source": { "type": "string", "description": "Source origin: manual, code_extract, design, etc. (optional)" },
                "node_kind": { "type": "string", "description": "Specific classification within node_type (optional)" },
                "file_path": { "type": "string", "description": "Associated file path for code nodes (optional)" },
                "metadata": { "type": "object", "description": "Additional metadata key-value pairs (optional)" },
                proj_key: proj_val,
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

        if let Some(nt) = input["node_type"].as_str() { node.node_type = Some(nt.to_string()); }
        if let Some(src) = input["source"].as_str() { node.source = Some(src.to_string()); }
        if let Some(nk) = input["node_kind"].as_str() { node.node_kind = Some(nk.to_string()); }
        if let Some(fp) = input["file_path"].as_str() { node.file_path = Some(fp.to_string()); }
        if let Some(meta) = input["metadata"].as_object() {
            for (k, v) in meta { node.metadata.insert(k.clone(), v.clone()); }
        }

        let (graph_arc, path) = self.mgr.resolve(&input).await?;
        let mut graph = graph_arc.write().await;
        graph.add_node(node);

        // Add dependency edges
        if let Some(deps) = input["depends_on"].as_array() {
            for dep in deps {
                if let Some(dep_id) = dep.as_str() {
                    graph.add_edge(Edge::depends_on(id, dep_id));
                }
            }
        }

        save_gid_graph(&graph, &path)?;

        Ok(ToolResult {
            output: format!("✅ Task '{}' added: {}", id, title),
            is_error: false,
        })
    }
}

// ── gid_update_task: update task status/fields ──

struct GidUpdateTaskTool {
    mgr: Arc<GraphManager>,
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
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Task ID to update" },
                "status": { "type": "string", "enum": ["todo", "in_progress", "done", "blocked", "cancelled"], "description": "New status" },
                "title": { "type": "string", "description": "New title (optional)" },
                "description": { "type": "string", "description": "New description (optional)" },
                "tags": { "type": "array", "items": { "type": "string" }, "description": "Tags (optional)" },
                "metadata": { "type": "object", "description": "Additional metadata key-value pairs — shallow merge (optional)" },
                "priority": { "type": "integer", "description": "Priority 0-255 (0=highest, optional)" },
                "node_type": { "type": "string", "description": "Node type (optional)" },
                "node_kind": { "type": "string", "description": "Node kind (optional)" },
                proj_key: proj_val,
            },
            "required": ["id"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'id'"))?;

        let (graph_arc, path) = self.mgr.resolve(&input).await?;
        let mut graph = graph_arc.write().await;

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
        if let Some(tags) = input["tags"].as_array() {
            let new_tags: Vec<String> = tags.iter().filter_map(|v| v.as_str().map(String::from)).collect();
            changes.push(format!("tags → {:?}", new_tags));
            node.tags = new_tags;
        }
        if let Some(meta) = input["metadata"].as_object() {
            for (k, v) in meta { node.metadata.insert(k.clone(), v.clone()); }
            changes.push("metadata updated (shallow merge)".to_string());
        }
        if let Some(p) = input["priority"].as_u64() {
            changes.push(format!("priority → {}", p));
            node.priority = Some(p as u8);
        }
        if let Some(nt) = input["node_type"].as_str() {
            changes.push(format!("node_type → {}", nt));
            node.node_type = Some(nt.to_string());
        }
        if let Some(nk) = input["node_kind"].as_str() {
            changes.push(format!("node_kind → {}", nk));
            node.node_kind = Some(nk.to_string());
        }

        if changes.is_empty() {
            return Ok(ToolResult { output: format!("No changes for task '{}'", id), is_error: false });
        }

        save_gid_graph(&graph, &path)?;

        Ok(ToolResult {
            output: format!("✅ Task '{}' updated: {}", id, changes.join(", ")),
            is_error: false,
        })
    }
}

// ── gid_add_edge: add a dependency edge ──

struct GidAddEdgeTool {
    mgr: Arc<GraphManager>,
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
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "properties": {
                "from": { "type": "string", "description": "Source task ID" },
                "to": { "type": "string", "description": "Target task ID" },
                "relation": { "type": "string", "description": "Relationship type (default: depends_on). Common values: depends_on, blocks, subtask_of, relates_to, implements, contains, tests_for, calls, imports, defined_in, belongs_to, maps_to, overrides, inherits" },
                proj_key: proj_val,
            },
            "required": ["from", "to"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let from = input["from"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'from'"))?;
        let to = input["to"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'to'"))?;
        let relation = input["relation"].as_str().unwrap_or("depends_on");

        let (graph_arc, path) = self.mgr.resolve(&input).await?;
        let mut graph = graph_arc.write().await;
        graph.add_edge(Edge::new(from, to, relation));
        save_gid_graph(&graph, &path)?;

        Ok(ToolResult {
            output: format!("✅ Edge added: {} —[{}]→ {}", from, relation, to),
            is_error: false,
        })
    }
}

// ── Layer filter helper ──

/// Build a filtered sub-graph containing only nodes/edges matching the given layer.
/// - "project": project nodes + project edges (default for most commands)
/// - "code": code nodes + code edges
/// - "all" or None: everything (no filtering)
fn filter_graph_by_layer(graph: &Graph, layer: Option<&str>) -> Graph {
    match layer {
        Some("project") => {
            let node_ids: std::collections::HashSet<String> = graph.project_nodes().iter().map(|n| n.id.clone()).collect();
            let mut filtered = Graph::new();
            for n in graph.project_nodes() {
                filtered.add_node(n.clone());
            }
            for e in graph.project_edges() {
                if node_ids.contains(&e.from) && node_ids.contains(&e.to) {
                    filtered.add_edge(e.clone());
                }
            }
            filtered
        }
        Some("code") => {
            let node_ids: std::collections::HashSet<String> = graph.code_nodes().iter().map(|n| n.id.clone()).collect();
            let mut filtered = Graph::new();
            for n in graph.code_nodes() {
                filtered.add_node(n.clone());
            }
            for e in graph.code_edges() {
                if node_ids.contains(&e.from) && node_ids.contains(&e.to) {
                    filtered.add_edge(e.clone());
                }
            }
            filtered
        }
        _ => graph.clone(), // "all" or unspecified
    }
}

// ── gid_read: read full graph as YAML ──

struct GidReadTool {
    mgr: Arc<GraphManager>,
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
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "properties": {
                "layer": {
                    "type": "string",
                    "enum": ["project", "code", "all"],
                    "description": "Filter by layer: project (tasks/features), code (extracted code nodes), all (default: all)"
                },
                proj_key: proj_val,
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let (graph_arc, _path) = self.mgr.resolve(&input).await?;
        let graph = graph_arc.read().await;
        let layer = input["layer"].as_str();
        let filtered = filter_graph_by_layer(&graph, layer);
        let yaml = serde_yaml::to_string(&filtered)?;
        Ok(ToolResult { output: yaml, is_error: false })
    }
}

// ── gid_complete: mark task done and show unblocked tasks ──

struct GidCompleteTool {
    mgr: Arc<GraphManager>,
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
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Task ID to mark as done" },
                proj_key: proj_val,
            },
            "required": ["id"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'id'"))?;

        let (graph_arc, path) = self.mgr.resolve(&input).await?;
        let mut graph = graph_arc.write().await;

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

        save_gid_graph(&graph, &path)?;

        let mut output = format!("✅ Task '{}' marked done.", id);
        if !newly_unblocked.is_empty() {
            output.push_str(&format!("\n🔓 Now unblocked: {}", newly_unblocked.join(", ")));
        }

        Ok(ToolResult { output, is_error: false })
    }
}

// ── gid_query_impact: impact analysis ──

struct GidQueryImpactTool {
    mgr: Arc<GraphManager>,
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
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Task ID to analyze" },
                "relations": { "type": "array", "items": { "type": "string" }, "description": "Filter traversal by edge relations (e.g. depends_on, blocks, tests_for, implements, calls, imports). Default: all relations." },
                proj_key: proj_val,
            },
            "required": ["id"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'id'"))?;

        let (graph_arc, _path) = self.mgr.resolve(&input).await?;
        let graph = graph_arc.read().await;
        let engine = QueryEngine::new(&graph);

        let relations_input: Option<Vec<String>> = input["relations"].as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());
        let impacted = if let Some(ref rels) = relations_input {
            let rel_refs: Vec<&str> = rels.iter().map(|s| s.as_str()).collect();
            engine.impact_filtered(id, Some(&rel_refs))
        } else {
            engine.impact(id)
        };

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
    mgr: Arc<GraphManager>,
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
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Task ID to query" },
                "transitive": { "type": "boolean", "description": "Include transitive dependencies (default: true)" },
                "relations": { "type": "array", "items": { "type": "string" }, "description": "Filter traversal by edge relations (e.g. depends_on, blocks, tests_for, implements, calls, imports). Default: all relations." },
                proj_key: proj_val,
            },
            "required": ["id"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'id'"))?;
        let transitive = input["transitive"].as_bool().unwrap_or(true);

        let (graph_arc, _path) = self.mgr.resolve(&input).await?;
        let graph = graph_arc.read().await;
        let engine = QueryEngine::new(&graph);

        let relations_input: Option<Vec<String>> = input["relations"].as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());
        let deps = if let Some(ref rels) = relations_input {
            let rel_refs: Vec<&str> = rels.iter().map(|s| s.as_str()).collect();
            engine.deps_filtered(id, transitive, Some(&rel_refs))
        } else {
            engine.deps(id, transitive)
        };

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
    mgr: Arc<GraphManager>,
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
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "properties": {
                proj_key: proj_val,
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let (graph_arc, _path) = self.mgr.resolve(&input).await?;
        let graph = graph_arc.read().await;
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
    mgr: Arc<GraphManager>,
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
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "properties": {
                proj_key: proj_val,
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let (graph_arc, _path) = self.mgr.resolve(&input).await?;
        let graph = graph_arc.read().await;
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
    mgr: Arc<GraphManager>,
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
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "properties": {
                "format": {
                    "type": "string",
                    "enum": ["ascii", "dot", "mermaid"],
                    "description": "Output format (default: ascii)"
                },
                "layer": {
                    "type": "string",
                    "enum": ["project", "code", "all"],
                    "description": "Filter by layer: project (default — tasks/features only), code (extracted code nodes), all (everything)"
                },
                proj_key: proj_val,
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let format_str = input["format"].as_str().unwrap_or("ascii");
        let format = match format_str.parse::<VisualFormat>() {
            Ok(f) => f,
            Err(e) => return Ok(ToolResult { output: e.to_string(), is_error: true }),
        };

        let (graph_arc, _path) = self.mgr.resolve(&input).await?;
        let graph = graph_arc.read().await;
        let layer = input["layer"].as_str().or(Some("project")); // default: project
        let filtered = filter_graph_by_layer(&graph, layer);
        let output = render(&filtered, format);

        Ok(ToolResult { output, is_error: false })
    }
}

// ── gid_history: list/save snapshots ──

struct GidHistoryTool {
    mgr: Arc<GraphManager>,
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
        let (proj_key, proj_val) = gid_project_property();
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
                },
                proj_key: proj_val,
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let action = input["action"].as_str().unwrap_or("list");
        let (_graph_arc, path) = self.mgr.resolve(&input).await?;
        let graph_path = std::path::Path::new(path.as_str());
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
    mgr: Arc<GraphManager>,
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
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["rename", "merge", "update_title", "delete"],
                    "description": "Operation type"
                },
                "id": { "type": "string", "description": "Target node ID" },
                "new_id": { "type": "string", "description": "New ID (for rename)" },
                "new_title": { "type": "string", "description": "New title (for update_title)" },
                "merge_into": { "type": "string", "description": "Target node to merge into (for merge)" },
                "preview": { "type": "boolean", "description": "Preview only, don't apply (default: false)" },
                proj_key: proj_val,
            },
            "required": ["operation", "id"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let operation = input["operation"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'operation'"))?;
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'id'"))?;
        let preview = input["preview"].as_bool().unwrap_or(false);

        let (graph_arc, path) = self.mgr.resolve(&input).await?;
        let mut graph = graph_arc.write().await;

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
                save_gid_graph(&graph, &path)?;

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
                save_gid_graph(&graph, &path)?;

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
                save_gid_graph(&graph, &path)?;

                Ok(ToolResult {
                    output: format!("✅ Updated title for '{}': {}", id, new_title),
                    is_error: false,
                })
            }
            "delete" => {
                // Count edges before removal (remove_node cleans them up)
                let edge_count = graph.edges.iter().filter(|e| e.from == id || e.to == id).count();

                match graph.remove_node(id) {
                    Some(_removed) => {
                        save_gid_graph(&graph, &path)?;
                        Ok(ToolResult {
                            output: format!("✅ Deleted node '{}' and {} associated edges", id, edge_count),
                            is_error: false,
                        })
                    }
                    None => {
                        Ok(ToolResult {
                            output: format!("Node '{}' not found", id),
                            is_error: true,
                        })
                    }
                }
            }
            _ => Ok(ToolResult {
                output: format!("Unknown operation: {}. Use 'rename', 'merge', 'update_title', or 'delete'.", operation),
                is_error: true,
            }),
        }
    }
}

// ── gid_extract: extract code graph and merge with task graph ──

struct GidExtractTool {
    mgr: Arc<GraphManager>,
}

#[async_trait]
impl Tool for GidExtractTool {
    fn name(&self) -> &str {
        "gid_extract"
    }

    fn description(&self) -> &str {
        "Extract code structure from a directory and merge into the task graph. Analyzes source files to create nodes for files, classes, and functions."
    }

    fn input_schema(&self) -> Value {
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "properties": {
                "dir": {
                    "type": "string",
                    "description": "Directory to analyze (default: workspace src/)"
                },
                proj_key: proj_val,
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let dir = input["dir"].as_str().unwrap_or("src");
        let dir_path = std::path::Path::new(dir);

        if !dir_path.exists() {
            return Ok(ToolResult {
                output: format!("Directory not found: {}", dir),
                is_error: true,
            });
        }

        // Determine if this is an external directory (outside workspace).
        // If so, write to a separate .gid/graph.yml in that project instead of
        // merging into the workspace graph.
        let (graph_arc, path) = self.mgr.resolve(&input).await?;
        let workspace_graph_path = std::path::Path::new(path.as_str());
        let workspace_dir = workspace_graph_path
            .parent() // .gid/
            .and_then(|p| p.parent()) // workspace root
            .unwrap_or(std::path::Path::new("."));
        let canonical_dir = dir_path.canonicalize().unwrap_or_else(|_| dir_path.to_path_buf());
        let canonical_ws = workspace_dir.canonicalize().unwrap_or_else(|_| workspace_dir.to_path_buf());
        let is_external = !canonical_dir.starts_with(&canonical_ws);

        // Resolve project root and .gid/ directory for incremental extraction
        let project_root = if is_external {
            find_project_root(dir_path).unwrap_or_else(|| dir_path.to_path_buf())
        } else {
            workspace_dir.to_path_buf()
        };
        let gid_dir = project_root.join(".gid");
        std::fs::create_dir_all(&gid_dir)?;
        let meta_path = gid_dir.join("extract-meta.json");

        // Extract code graph incrementally (matches CLI behavior)
        let (mut code_graph, report) = match CodeGraph::extract_incremental(dir_path, &gid_dir, &meta_path, false) {
            Ok((cg, rpt)) => (cg, rpt.to_string()),
            Err(e) => {
                // Fallback to full extract if incremental fails
                tracing::warn!("Incremental extract failed ({}), falling back to full extract", e);
                let cg = CodeGraph::extract_from_dir(dir_path);
                let fallback_report = format!("Full rebuild (incremental failed: {})", e);
                (cg, fallback_report)
            }
        };

        // LSP refinement pass (default on, matches CLI behavior)
        let lsp_info = match code_graph.refine_with_lsp(&canonical_dir) {
            Ok(stats) => {
                let mut info = if stats.refined > 0 || stats.removed > 0 || stats.references_edges_added > 0 || stats.implementation_edges_added > 0 {
                    format!(
                        "\n  - LSP: {} refined, {} removed, {} ref edges, {} impl edges",
                        stats.refined, stats.removed,
                        stats.references_edges_added, stats.implementation_edges_added,
                    )
                } else {
                    "\n  - LSP: no changes needed".to_string()
                };
                if !stats.missing_servers.is_empty() {
                    let langs: Vec<&str> = stats.missing_servers.iter().map(|m| m.language_id.as_str()).collect();
                    info.push_str(&format!("\n  - ⚠️ Missing LSP servers: {}", langs.join(", ")));
                }
                info
            }
            Err(e) => {
                format!("\n  - LSP refinement skipped: {}", e)
            }
        };

        let code_nodes = code_graph.nodes.len();
        let code_edges = code_graph.edges.len();

        if is_external {
            // External project: write to its own .gid/
            let ext_backend = detect_backend(&gid_dir);
            let target_graph_path = match ext_backend {
                StorageBackend::Sqlite => gid_dir.join("graph.db"),
                _ => gid_dir.join("graph.yml"),
            };

            // Load existing graph from that project (or empty)
            let existing = load_graph_auto(&gid_dir, Some(ext_backend)).unwrap_or_default();
            let existing_nodes = existing.nodes.len();
            let existing_edges = existing.edges.len();

            // Build unified graph
            let (code_nodes_vec, code_edges_vec) = codegraph_to_graph_nodes(&code_graph, &project_root);
            let mut unified = existing;
            merge_code_layer(&mut unified, code_nodes_vec, code_edges_vec);
            let total_nodes = unified.nodes.len();
            let total_edges = unified.edges.len();

            // Save to the external project's .gid/
            save_graph_auto(&unified, &gid_dir, Some(ext_backend))
                .map_err(|e| anyhow::anyhow!("{}", e))?;

            Ok(ToolResult {
                output: format!(
                    "✅ Code extraction complete (external project):\n  - Analyzed: {}\n  - Extract: {}\n  - Found: {} code nodes, {} edges{}\n  - Existing graph: {} nodes, {} edges\n  - New unified: {} nodes, {} edges\n  - Saved to: {}",
                    dir, report, code_nodes, code_edges, lsp_info,
                    existing_nodes, existing_edges,
                    total_nodes, total_edges,
                    target_graph_path.display()
                ),
                is_error: false,
            })
        } else {
            // Internal: merge into workspace graph
            let mut graph = graph_arc.write().await;
            let existing_nodes = graph.nodes.len();
            let existing_edges = graph.edges.len();

            let (code_nodes_vec, code_edges_vec) = codegraph_to_graph_nodes(&code_graph, workspace_dir);
            merge_code_layer(&mut graph, code_nodes_vec, code_edges_vec);
            save_gid_graph(&graph, &path)?;

            let new_nodes = graph.nodes.len() - existing_nodes;
            let new_edges = graph.edges.len() - existing_edges;

            Ok(ToolResult {
                output: format!(
                    "✅ Code extraction complete:\n  - Analyzed: {}\n  - Extract: {}\n  - Found: {} code nodes, {} edges{}\n  - Existing graph: {} nodes, {} edges\n  - New unified: {} nodes, {} edges\n  - Added: {} nodes, {} edges",
                    dir, report, code_nodes, code_edges, lsp_info,
                    existing_nodes, existing_edges,
                    graph.nodes.len(), graph.edges.len(),
                    new_nodes, new_edges
                ),
                is_error: false,
            })
        }
    }
}

/// Walk up from `dir` to find the project root (directory containing .git, Cargo.toml, package.json, etc.)
///
/// For Rust workspaces: keeps walking up past sub-crate Cargo.toml files to find the
/// workspace root (the one with `[workspace]`). For monorepos: prefers `.git` over
/// individual package markers.
fn find_project_root(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let mut current = if canonical.is_file() {
        canonical.parent()?.to_path_buf()
    } else {
        canonical
    };

    // Strong markers that definitively identify a project root.
    let strong_markers = [".git"];
    // Weak markers that might appear in sub-directories of a larger project.
    let weak_markers = ["Cargo.toml", "package.json", "pyproject.toml", "go.mod", ".gid"];

    let mut first_weak_match: Option<std::path::PathBuf> = None;

    loop {
        // Strong marker = definitive root, return immediately.
        for marker in &strong_markers {
            if current.join(marker).exists() {
                return Some(current);
            }
        }

        // Weak marker = candidate, but keep looking for a strong marker or workspace root.
        if first_weak_match.is_none() {
            for marker in &weak_markers {
                if current.join(marker).exists() {
                    first_weak_match = Some(current.clone());
                    break;
                }
            }
        }

        // Check if this is a Rust workspace root (Cargo.toml with [workspace]).
        let cargo_path = current.join("Cargo.toml");
        if cargo_path.exists() {
            if let Ok(contents) = std::fs::read_to_string(&cargo_path) {
                if contents.contains("[workspace]") {
                    return Some(current);
                }
            }
        }

        // Check if this is a Node.js monorepo root (package.json with "workspaces").
        let pkg_path = current.join("package.json");
        if pkg_path.exists() {
            if let Ok(contents) = std::fs::read_to_string(&pkg_path) {
                if contents.contains("\"workspaces\"") {
                    return Some(current);
                }
            }
        }

        if !current.pop() {
            break;
        }
    }

    // No strong marker found — return first weak match.
    first_weak_match
}

/// Load CodeGraph from graph.yml first, falling back to extract_from_dir.
fn load_code_graph_for_tool(dir: &std::path::Path) -> CodeGraph {
    // Try graph.yml first
    let project_root = find_project_root(dir).unwrap_or_else(|| dir.to_path_buf());
    let graph_yml = project_root.join(".gid").join("graph.yml");
    if graph_yml.exists() {
        if let Ok(yaml_str) = std::fs::read_to_string(&graph_yml) {
            if let Ok(graph) = serde_yaml::from_str::<Graph>(&yaml_str) {
                let code_graph = graph_to_codegraph(&graph);
                if !code_graph.nodes.is_empty() {
                    return code_graph;
                }
            }
        }
    }
    // Fallback to extraction
    CodeGraph::extract_from_dir(dir)
}

// ── gid_schema: get code schema (classes, functions, signatures) ──

struct GidSchemaTool {
    mgr: Arc<GraphManager>,
}

#[async_trait]
impl Tool for GidSchemaTool {
    fn name(&self) -> &str {
        "gid_schema"
    }

    fn description(&self) -> &str {
        "Extract and return the code schema (classes, functions, signatures) from a directory."
    }

    fn input_schema(&self) -> Value {
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "properties": {
                "dir": {
                    "type": "string",
                    "description": "Directory to analyze (required)"
                },
                proj_key: proj_val,
            },
            "required": ["dir"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let dir = input["dir"].as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'dir' parameter"))?;
        let dir_path = std::path::Path::new(dir);

        if !dir_path.exists() {
            return Ok(ToolResult {
                output: format!("Directory not found: {}", dir),
                is_error: true,
            });
        }

        // Try graph.yml code nodes first
        let (graph_arc, _path) = self.mgr.resolve(&input).await?;
        let graph = graph_arc.read().await;
        let code_nodes = graph.code_nodes();
        if !code_nodes.is_empty() {
            // Build schema from graph code nodes
            let mut output = String::new();
            let mut files: std::collections::BTreeMap<String, Vec<&Node>> = std::collections::BTreeMap::new();
            for node in &code_nodes {
                let file = node.file_path.as_deref().unwrap_or("unknown");
                files.entry(file.to_string()).or_default().push(node);
            }
            for (file, nodes) in &files {
                output.push_str(&format!("## {}\n", file));
                for n in nodes {
                    let kind = n.node_type.as_deref().unwrap_or("item");
                    output.push_str(&format!("  {} {} {}\n", kind, n.id, n.title));
                }
                output.push('\n');
            }
            return Ok(ToolResult {
                output: format!("(from graph.yml code nodes — {} nodes, {} files)\n\n{}", code_nodes.len(), files.len(), output),
                is_error: false,
            });
        }
        drop(graph);

        // Fallback: extract from source (prefer graph.yml)
        let code_graph = load_code_graph_for_tool(dir_path);
        let schema = code_graph.get_schema();

        if schema.is_empty() {
            return Ok(ToolResult {
                output: format!("No code structure found in: {}", dir),
                is_error: false,
            });
        }

        Ok(ToolResult {
            output: schema,
            is_error: false,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// GID Design Tool — AI-assisted graph generation from design docs
// ═══════════════════════════════════════════════════════════════════════════════

struct GidDesignTool {
    mgr: Arc<GraphManager>,
}

#[async_trait]
impl Tool for GidDesignTool {
    fn name(&self) -> &str {
        "gid_design"
    }

    fn description(&self) -> &str {
        "Generate a graph design prompt from the current graph, or parse YAML output into graph nodes/edges. Use --parse to merge generated YAML into the graph."
    }

    fn input_schema(&self) -> Value {
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "properties": {
                "parse": {
                    "type": "boolean",
                    "description": "If true, parse YAML from 'yaml_content' and merge into graph"
                },
                "yaml_content": {
                    "type": "string",
                    "description": "YAML content to parse (required when parse=true)"
                },
                "context": {
                    "type": "string",
                    "description": "Additional context for design prompt generation"
                },
                proj_key: proj_val,
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let parse = input["parse"].as_bool().unwrap_or(false);

        if parse {
            let yaml_content = input["yaml_content"].as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'yaml_content' for --parse mode"))?;

            // Parse the YAML into nodes and edges
            let parsed: serde_yaml::Value = serde_yaml::from_str(yaml_content)
                .map_err(|e| anyhow::anyhow!("Failed to parse YAML: {}", e))?;

            let (graph_arc, path) = self.mgr.resolve(&input).await?;
            let mut graph = graph_arc.write().await;
            let mut added_nodes = 0;
            let mut added_edges = 0;

            // Parse nodes
            if let Some(nodes) = parsed.get("nodes").and_then(|n| n.as_sequence()) {
                for node_val in nodes {
                    if let Ok(node) = serde_yaml::from_value::<Node>(node_val.clone()) {
                        if graph.get_node(&node.id).is_none() {
                            graph.add_node(node);
                            added_nodes += 1;
                        }
                    }
                }
            }

            // Parse edges
            if let Some(edges) = parsed.get("edges").and_then(|e| e.as_sequence()) {
                for edge_val in edges {
                    if let Ok(edge) = serde_yaml::from_value::<Edge>(edge_val.clone()) {
                        graph.add_edge(edge);
                        added_edges += 1;
                    }
                }
            }

            // Save
            save_gid_graph(&graph, &path)?;

            Ok(ToolResult {
                output: format!("Merged: {} nodes added, {} edges added", added_nodes, added_edges),
                is_error: false,
            })
        } else {
            // Generate design prompt
            let (graph_arc, _path) = self.mgr.resolve(&input).await?;
            let graph = graph_arc.read().await;
            let context = input["context"].as_str().unwrap_or("");

            let node_count = graph.nodes.len();
            let edge_count = graph.edges.len();

            let prompt = format!(
                "Generate graph nodes and edges in YAML format for the following project.\n\n\
                 Current graph has {} nodes and {} edges.\n\n\
                 Context: {}\n\n\
                 Output format:\n\
                 ```yaml\n\
                 nodes:\n\
                 - id: <id>\n\
                   title: <description>\n\
                   status: todo\n\
                   tags: [<tag>]\n\
                 edges:\n\
                 - from: <id>\n\
                   to: <id>\n\
                   relation: depends_on\n\
                 ```\n\n\
                 Then call gid_design with parse=true and the YAML content.",
                node_count, edge_count, context
            );

            Ok(ToolResult {
                output: prompt,
                is_error: false,
            })
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// GID Plan Tool — Show execution plan from graph
// ═══════════════════════════════════════════════════════════════════════════════

struct GidPlanTool {
    mgr: Arc<GraphManager>,
}

#[async_trait]
impl Tool for GidPlanTool {
    fn name(&self) -> &str {
        "gid_plan"
    }

    fn description(&self) -> &str {
        "Create an execution plan from the current graph. Shows layers, task ordering, and parallelism. Use detail=true for critical path and turn estimates."
    }

    fn input_schema(&self) -> Value {
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "properties": {
                "detail": { "type": "boolean", "description": "If true, include critical path analysis and estimated turns (default: false)" },
                proj_key: proj_val,
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        use gid_core::harness::create_plan;

        let detail = input["detail"].as_bool().unwrap_or(false);
        let (graph_arc, _path) = self.mgr.resolve(&input).await?;
        let graph = graph_arc.read().await;
        let plan = create_plan(&graph)?;

        let mut output = format!("Execution Plan: {} tasks in {} layers\n\n", plan.total_tasks, plan.layers.len());

        for (i, layer) in plan.layers.iter().enumerate() {
            output.push_str(&format!("Layer {} ({} tasks, parallel):\n", i, layer.tasks.len()));
            for task_info in &layer.tasks {
                output.push_str(&format!("  - {} — {}\n", task_info.id, task_info.title));
            }
            output.push('\n');
        }

        if detail {
            if !plan.critical_path.is_empty() {
                output.push_str("Critical Path:\n");
                for task_id in &plan.critical_path {
                    output.push_str(&format!("  → {}\n", task_id));
                }
                output.push('\n');
            }
            output.push_str(&format!("Estimated total turns: {}\n", plan.estimated_total_turns));
        }

        Ok(ToolResult {
            output,
            is_error: false,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// GID Execute Stats Tool — Show execution statistics from log
// ═══════════════════════════════════════════════════════════════════════════════

struct GidStatsTool {
    mgr: Arc<GraphManager>,
}

#[async_trait]
impl Tool for GidStatsTool {
    fn name(&self) -> &str {
        "gid_stats"
    }

    fn description(&self) -> &str {
        "Show execution statistics from the most recent harness run (tasks completed/failed, tokens, duration)."
    }

    fn input_schema(&self) -> Value {
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "properties": {
                proj_key: proj_val,
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        use gid_core::harness::TelemetryLogger;

        let gid_dir = self.mgr.resolve_gid_dir(&input).await?;
        let logger = TelemetryLogger::new(&gid_dir);
        let stats = logger.compute_stats()?;

        let output = format!(
            "Execution Stats:\n\
             Tasks completed: {}\n\
             Tasks failed: {}\n\
             Total turns: {}\n\
             Avg turns/task: {:.1}\n\
             Total tokens: {}\n\
             Duration: {}s",
            stats.tasks_completed,
            stats.tasks_failed,
            stats.total_turns,
            stats.avg_turns_per_task,
            stats.total_tokens,
            stats.duration_secs,
        );

        Ok(ToolResult {
            output,
            is_error: false,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// GID Semantify Tool — Upgrade file-level graph to semantic graph
// ═══════════════════════════════════════════════════════════════════════════════

struct GidSemantifyTool {
    mgr: Arc<GraphManager>,
}

#[async_trait]
impl Tool for GidSemantifyTool {
    fn name(&self) -> &str {
        "gid_semantify"
    }

    fn description(&self) -> &str {
        "Upgrade a file-level graph to a semantic graph by assigning architectural layers (interface, application, domain, infrastructure) to nodes based on file paths. Uses heuristics — no LLM call required."
    }

    fn input_schema(&self) -> Value {
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "properties": {
                proj_key: proj_val,
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let (graph_arc, path) = self.mgr.resolve(&input).await?;
        let mut graph = graph_arc.write().await;
        
        let assigned = semantify::apply_heuristic_layers(&mut graph);
        
        // Save the updated graph
        save_gid_graph(&graph, &path)?;

        let mut output = format!("✓ Semantify complete: {} nodes assigned layers\n\n", assigned);
        
        // Show layer distribution
        let mut layer_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for node in &graph.nodes {
            if let Some(layer) = node.metadata.get("layer").and_then(|v| v.as_str()) {
                *layer_counts.entry(layer.to_string()).or_default() += 1;
            }
        }
        
        if !layer_counts.is_empty() {
            output.push_str("Layer distribution:\n");
            for (layer, count) in &layer_counts {
                output.push_str(&format!("  • {}: {} nodes\n", layer, count));
            }
        }

        Ok(ToolResult {
            output,
            is_error: false,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// GID Complexity Tool — Code complexity/risk analysis
// ═══════════════════════════════════════════════════════════════════════════════

struct GidComplexityTool;

#[async_trait]
impl Tool for GidComplexityTool {
    fn name(&self) -> &str {
        "gid_complexity"
    }

    fn description(&self) -> &str {
        "Analyze code complexity and risk from the code graph. Examines relevant nodes, inheritance depth, import edges, and test coverage to classify as simple/medium/complex."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "dir": {
                    "type": "string",
                    "description": "Directory to analyze (default: src)"
                },
                "keywords": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Keywords to focus analysis on (e.g., ['auth', 'login'])"
                },
                "problem": {
                    "type": "string",
                    "description": "Problem statement to extract keywords from (alternative to keywords)"
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let dir = input["dir"].as_str().unwrap_or("src");
        let dir_path = std::path::Path::new(dir);
        
        if !dir_path.exists() {
            return Ok(ToolResult {
                output: format!("Directory not found: {}", dir),
                is_error: true,
            });
        }
        
        // Extract code graph (prefer graph.yml)
        let code_graph = load_code_graph_for_tool(dir_path);
        
        // Get keywords
        let keywords: Vec<&str> = if let Some(kw_array) = input["keywords"].as_array() {
            kw_array.iter().filter_map(|v| v.as_str()).collect()
        } else if let Some(problem) = input["problem"].as_str() {
            let extracted = CodeGraph::extract_keywords(problem);
            extracted.into_iter().collect()
        } else {
            vec!["main", "core", "lib"]
        };
        
        // Assess complexity
        let report = complexity::assess_complexity_from_graph(&code_graph, &keywords, 0);
        
        let mut output = format!(
            "Complexity Analysis\n\
             ═══════════════════\n\
             Complexity: {:?}\n\
             Relevant nodes: {}\n\
             Relevant files: {}\n\
             Classes: {}\n\
             Inheritance edges: {}\n\
             Import edges: {}\n\
             Tests: {}\n\n\
             Summary: {}\n",
            report.complexity,
            report.relevant_nodes,
            report.relevant_files,
            report.class_count,
            report.inheritance_edges,
            report.import_edges,
            report.test_count,
            report.summary,
        );
        
        // Add risk assessment if we have node IDs
        if report.relevant_nodes > 0 {
            let risk = complexity::assess_risk_level(&code_graph, &[]);
            output.push_str(&format!("\nRisk level: {}\n", risk));
        }

        Ok(ToolResult {
            output,
            is_error: false,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// GID Working Memory Tool — Changed files → affected nodes
// ═══════════════════════════════════════════════════════════════════════════════

struct GidWorkingMemoryTool;

#[async_trait]
impl Tool for GidWorkingMemoryTool {
    fn name(&self) -> &str {
        "gid_working_memory"
    }

    fn description(&self) -> &str {
        "Analyze impact of changed files on the codebase. Shows affected source nodes, related tests, risk level, and blast radius."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "files": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "List of changed file paths to analyze"
                },
                "dir": {
                    "type": "string",
                    "description": "Project source directory for code graph extraction (default: src)"
                }
            },
            "required": ["files"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let files: Vec<String> = input["files"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing 'files' parameter"))?
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        
        if files.is_empty() {
            return Ok(ToolResult {
                output: "No files provided to analyze.".to_string(),
                is_error: true,
            });
        }
        
        let dir = input["dir"].as_str().unwrap_or("src");
        let dir_path = std::path::Path::new(dir);
        
        // Extract code graph (prefer graph.yml)
        let code_graph = if dir_path.exists() {
            load_code_graph_for_tool(dir_path)
        } else {
            CodeGraph::default()
        };
        
        // Convert CodeGraph to Graph for analyze_impact
        let (code_nodes_vec, code_edges_vec) = codegraph_to_graph_nodes(&code_graph, dir_path);
        let mut unified = Graph::default();
        merge_code_layer(&mut unified, code_nodes_vec, code_edges_vec);
        
        // Analyze impact
        let analysis = working_mem::analyze_impact(&files, &unified);
        
        let mut output = format!(
            "Impact Analysis\n\
             ═══════════════\n\
             {}\n\n\
             Risk Level: {}\n\n",
            analysis.summary,
            analysis.risk_level,
        );
        
        if !analysis.affected_source.is_empty() {
            output.push_str("Affected Source Nodes:\n");
            for node in analysis.affected_source.iter().take(10) {
                output.push_str(&format!(
                    "  • {} ({}) — {} callers\n",
                    node.name, node.kind, node.callers
                ));
            }
            if analysis.affected_source.len() > 10 {
                output.push_str(&format!("  ... and {} more\n", analysis.affected_source.len() - 10));
            }
            output.push('\n');
        }
        
        if !analysis.affected_tests.is_empty() {
            output.push_str("Related Tests:\n");
            for node in analysis.affected_tests.iter().take(10) {
                output.push_str(&format!("  • {} ({})\n", node.name, node.file));
            }
            if analysis.affected_tests.len() > 10 {
                output.push_str(&format!("  ... and {} more\n", analysis.affected_tests.len() - 10));
            }
        }

        Ok(ToolResult {
            output,
            is_error: false,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// GID Ignore Tool — Check if path is ignored by .gidignore
// ═══════════════════════════════════════════════════════════════════════════════

struct GidIgnoreTool {
    mgr: Arc<GraphManager>,
}

#[async_trait]
impl Tool for GidIgnoreTool {
    fn name(&self) -> &str {
        "gid_ignore"
    }

    fn description(&self) -> &str {
        "Check if a path is ignored by .gidignore rules. Shows loaded patterns and whether a specific path would be ignored."
    }

    fn input_schema(&self) -> Value {
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to check (optional - if omitted, shows all ignore patterns)"
                },
                "is_dir": {
                    "type": "boolean",
                    "description": "Whether the path is a directory (default: false)"
                },
                proj_key: proj_val,
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        // Load ignore list from project directory (parent of .gid)
        let gid_dir = self.mgr.resolve_gid_dir(&input).await?;
        let project_dir = gid_dir.parent().unwrap_or(&gid_dir);
        let ignore_list = ignore::load_ignore_list(project_dir);
        
        let path_to_check = input["path"].as_str();
        let is_dir = input["is_dir"].as_bool().unwrap_or(false);
        
        let mut output = String::new();
        
        // Show loaded patterns
        output.push_str(&format!(
            "Ignore Patterns ({} total)\n═══════════════════════════\n",
            ignore_list.patterns().len()
        ));
        
        // Group by negated/normal
        let normal: Vec<_> = ignore_list.patterns().iter()
            .filter(|p| !p.negated)
            .take(20)
            .collect();
        let negated: Vec<_> = ignore_list.patterns().iter()
            .filter(|p| p.negated)
            .collect();
        
        if !normal.is_empty() {
            output.push_str("Ignored:\n");
            for p in &normal {
                let dir_marker = if p.dir_only { "/" } else { "" };
                output.push_str(&format!("  • {}{}\n", p.pattern, dir_marker));
            }
            if ignore_list.patterns().len() > 20 {
                output.push_str(&format!("  ... and {} more\n", ignore_list.patterns().len() - 20));
            }
        }
        
        if !negated.is_empty() {
            output.push_str("\nExceptions (not ignored):\n");
            for p in negated {
                output.push_str(&format!("  • !{}\n", p.pattern));
            }
        }
        
        // Check specific path if provided
        if let Some(path) = path_to_check {
            let ignored = ignore_list.should_ignore(path, is_dir);
            output.push_str(&format!(
                "\nPath check: {}\n  → {} {}\n",
                path,
                if ignored { "❌ IGNORED" } else { "✓ NOT ignored" },
                if is_dir { "(directory)" } else { "(file)" }
            ));
        }

        Ok(ToolResult {
            output,
            is_error: false,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// GID Scope Tool — Show ToolScope for ritual phase
// ═══════════════════════════════════════════════════════════════════════════════

struct GidScopeTool;

#[async_trait]
impl Tool for GidScopeTool {
    fn name(&self) -> &str {
        "gid_scope"
    }

    fn description(&self) -> &str {
        "Show the ToolScope for a ritual phase. Displays allowed tools, writable paths, and bash policy for the given phase."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "phase": {
                    "type": "string",
                    "description": "Phase ID (e.g., 'research', 'execute-tasks', 'verify-quality'). If omitted, shows all known phases."
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let phase = input["phase"].as_str();
        
        let mut output = String::new();
        
        let format_scope = |scope: &ToolScope, phase_id: &str| {
            let mut s = format!("Phase: {}\n", phase_id);
            s.push_str(&format!("  Tools: [{}]\n", scope.allowed_tools.join(", ")));
            s.push_str(&format!("  Writable: [{}]\n", scope.writable_paths.join(", ")));
            if !scope.readable_paths.is_empty() {
                s.push_str(&format!("  Readable: [{}]\n", scope.readable_paths.join(", ")));
            } else {
                s.push_str("  Readable: [all]\n");
            }
            s.push_str(&format!("  Bash: {:?}\n", scope.bash_policy));
            s
        };
        
        if let Some(phase_id) = phase {
            let scope = default_scope_for_phase(phase_id);
            output = format!("Tool Scope\n══════════\n\n{}", format_scope(&scope, phase_id));
        } else {
            // Show all known phases
            output.push_str("Tool Scopes for Known Phases\n════════════════════════════\n\n");
            let phases = [
                "capture-idea", "research", "draft-requirements", "draft-design",
                "generate-graph", "plan-tasks", "execute-tasks", "extract-code", "verify-quality"
            ];
            for phase_id in phases {
                let scope = default_scope_for_phase(phase_id);
                output.push_str(&format_scope(&scope, phase_id));
                output.push('\n');
            }
        }

        Ok(ToolResult {
            output,
            is_error: false,
        })
    }
}

// ── gid_infer: run infer pipeline (clustering → labeling → merge) ──

/// Bridge: gid-core's SimpleLlm trait → RustClaw's LlmClient.
/// Uses the existing Anthropic client (with auth, connection pool, retries)
/// instead of shelling out to `claude` CLI which deadlocks in non-tty environments.
struct RustClawSimpleLlm {
    client: Arc<tokio::sync::RwLock<Box<dyn crate::llm::LlmClient>>>,
}

#[async_trait]
impl infer::SimpleLlm for RustClawSimpleLlm {
    async fn complete(&self, prompt: &str) -> anyhow::Result<String> {
        use crate::llm::Message;
        // Eagerly log that complete() was called
        let debug_path = "/tmp/rustclaw-infer-debug.log";
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_path)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "\n=== COMPLETE CALLED (prompt: {} chars) ===", prompt.len())
            });
        let client = self.client.read().await;
        let messages = vec![Message::text("user", prompt)];
        let response = match client.chat_with_model("", &messages, &[], "claude-sonnet-4-5-20250929").await {
            Ok(r) => r,
            Err(e) => {
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(debug_path)
                    .and_then(|mut f| {
                        use std::io::Write;
                        writeln!(f, "=== LLM ERROR: {:?} ===", e)
                    });
                return Err(e);
            }
        };
        let text = response.text.unwrap_or_default();
        // Debug: write LLM exchange to file for debugging
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_path)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "=== RESPONSE ({} chars) ===\n{}\n=== END ===",
                    text.len(), text)
            });
        Ok(text)
    }
}

struct GidInferTool {
    mgr: Arc<GraphManager>,
    llm_client: Option<Arc<tokio::sync::RwLock<Box<dyn crate::llm::LlmClient>>>>,
}

#[async_trait]
impl Tool for GidInferTool {
    fn name(&self) -> &str {
        "gid_infer"
    }

    fn description(&self) -> &str {
        "Run the infer pipeline on a code graph: Infomap clustering → optional LLM labeling → component/feature nodes. Enriches the graph with architectural layers automatically discovered from code structure."
    }

    fn input_schema(&self) -> Value {
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "properties": {
                "dir": {
                    "type": "string",
                    "description": "Source directory for auto-extract if graph has no code nodes (optional)"
                },
                "level": {
                    "type": "string",
                    "description": "Inference level: 'component' (clustering only, no LLM), 'feature' (clustering + LLM labeling), 'all' (same as feature). Default: component",
                    "enum": ["component", "feature", "all"]
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "If true, show results without merging into graph (default: false)"
                },
                "format": {
                    "type": "string",
                    "description": "Output format for dry-run preview file: 'summary', 'yaml', 'json'. Default: summary",
                    "enum": ["summary", "yaml", "json"]
                },
                "min_community_size": {
                    "type": "integer",
                    "description": "Minimum files per component (default: 2)"
                },
                "hierarchical": {
                    "type": "boolean",
                    "description": "Use hierarchical clustering (default: false)"
                },
                "max_cluster_size": {
                    "type": "integer",
                    "description": "Maximum files per component. Clusters exceeding this are sub-clustered. Default: auto = max(20, total_files/5)"
                },
                "min_naming_size": {
                    "type": "integer",
                    "description": "Minimum files in a component to use LLM naming (smaller ones use auto-name). Default: 3"
                },
                "rollback_batch": {
                    "type": "string",
                    "description": "Batch ID to rollback (removes all infer nodes from that batch)"
                },
                proj_key: proj_val,
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        use infer::ClusterConfig;

        // Handle rollback first
        if let Some(batch_id) = input["rollback_batch"].as_str() {
            let (graph_arc, path) = self.mgr.resolve(&input).await?;
            let mut graph = graph_arc.write().await;
            let (nodes_removed, edges_removed) = rollback_infer_batch(&mut graph, batch_id);
            save_gid_graph(&graph, &path)?;
            return Ok(ToolResult {
                output: format!(
                    "🔄 Rollback batch '{}': removed {} nodes, {} edges\nSaved to: {}",
                    batch_id, nodes_removed, edges_removed, path
                ),
                is_error: false,
            });
        }

        let level = match input["level"].as_str().unwrap_or("component") {
            "component" => InferLevel::Component,
            "feature" => InferLevel::Feature,
            "all" => InferLevel::All,
            other => return Ok(ToolResult {
                output: format!("Unknown level '{}'. Use: component, feature, all", other),
                is_error: true,
            }),
        };

        let format = match input["format"].as_str().unwrap_or("summary") {
            "summary" => InferOutputFormat::Summary,
            "yaml" => InferOutputFormat::Yaml,
            "json" => InferOutputFormat::Json,
            other => return Ok(ToolResult {
                output: format!("Unknown format '{}'. Use: summary, yaml, json", other),
                is_error: true,
            }),
        };

        let dry_run = input["dry_run"].as_bool().unwrap_or(false);
        let source_dir = input["dir"].as_str().map(std::path::PathBuf::from);

        let mut cluster_config = ClusterConfig::default();
        if let Some(min) = input["min_community_size"].as_u64() {
            cluster_config.min_community_size = min as usize;
        }
        if let Some(h) = input["hierarchical"].as_bool() {
            cluster_config.hierarchical = h;
        }
        if let Some(max) = input["max_cluster_size"].as_u64() {
            cluster_config.max_cluster_size = Some(max as usize);
        }

        let labeling_config = if level == InferLevel::Component {
            None
        } else {
            let lc = infer::LabelingConfig::default();
            // min_naming_size removed from LabelingConfig in gid-core 0.3.2
            Some(lc)
        };

        let config = InferConfig {
            clustering: cluster_config,
            labeling: labeling_config,
            level,
            format,
            dry_run,
            source_dir,
        };

        // Run infer on a snapshot of the graph (infer::run doesn't mutate)
        let (graph_arc, path) = self.mgr.resolve(&input).await?;
        let graph = graph_arc.read().await;

        // Build LLM client for labeling — uses RustClaw's existing Anthropic client
        // (ISS-013: replaces CliLlm which deadlocked in non-tty environments)
        let llm_client: Option<RustClawSimpleLlm> = if level == InferLevel::Component {
            let _ = std::fs::write("/tmp/rustclaw-infer-debug.log", "LLM skipped: level=Component\n");
            None
        } else if let Some(ref client) = self.llm_client {
            let _ = std::fs::write("/tmp/rustclaw-infer-debug.log", "LLM client found, creating RustClawSimpleLlm\n");
            Some(RustClawSimpleLlm { client: client.clone() })
        } else {
            let _ = std::fs::write("/tmp/rustclaw-infer-debug.log", "LLM client is NONE! bail!\n");
            anyhow::bail!("Feature-level inference requires LLM but no LLM client configured");
        };
        let result = infer::run(&graph, &config, llm_client.as_ref().map(|c| c as &dyn infer::SimpleLlm)).await.map_err(|e| {
            anyhow::anyhow!("Infer pipeline failed: {}", e)
        })?;
        drop(graph);

        // Merge if not dry-run
        if !dry_run {
            let mut graph = graph_arc.write().await;
            let stats = merge_into_graph(&mut graph, &result, true);
            save_gid_graph(&graph, &path)?;

            // Count code files (approximate - count unique "contains" edges from components)
            let code_file_count = result.edges.iter()
                .filter(|e| e.relation == "contains")
                .map(|e| e.to.as_str())
                .collect::<std::collections::HashSet<_>>()
                .len();

            Ok(ToolResult {
                output: format!(
                    "✅ Infer complete (batch: {})\n\
                     • {} components, {} features merged\n\
                     • {} edges added\n\
                     • {} old nodes removed, {} skipped (user-owned)\n\
                     • {} code files clustered\n\
                     • Saved to: {}\n\
                     \nUse gid_read or gid_tasks to inspect results. To rollback: gid_infer with rollback_batch=\"{}\"",
                    stats.batch_id,
                    stats.components_added, stats.features_added,
                    stats.edges_added,
                    stats.old_nodes_removed, stats.nodes_skipped,
                    code_file_count,
                    path,
                    stats.batch_id,
                ),
                is_error: false,
            })
        } else {
            // Write full output to file for human/LLM review
            let preview_ext = match config.format {
                InferOutputFormat::Yaml => "yml",
                InferOutputFormat::Json => "json",
                InferOutputFormat::Summary => "txt",
            };
            let preview_path = {
                let gid_dir = std::path::Path::new(&*path).parent().unwrap_or(std::path::Path::new("."));
                gid_dir.join(format!("infer-preview.{}", preview_ext))
            };
            let output_text = infer_format_output(&result, config.format);
            std::fs::write(&preview_path, &output_text)?;

            // Return concise summary
            let num_components = result.component_nodes.len();
            let num_features = result.feature_nodes.len();
            let num_edges = result.edges.len();

            Ok(ToolResult {
                output: format!(
                    "🔍 Dry-run complete (no changes written)\n\
                     • {} components, {} features discovered\n\
                     • {} edges\n\
                     • Clustering: {} communities, codelength = {:.3}\n\
                     \nFull preview written to: {}\n\
                     Use read_file to inspect if needed. Run without dry_run to merge.",
                    num_components, num_features,
                    num_edges,
                    result.cluster_metrics.num_communities,
                    result.cluster_metrics.codelength,
                    preview_path.display(),
                ),
                is_error: false,
            })
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Start Ritual Tool — V2 ritual entry point for LLM
// ═══════════════════════════════════════════════════════════════════════════════

struct StartRitualTool {
    workspace_root: PathBuf,
    llm_client: Option<Arc<tokio::sync::RwLock<Box<dyn crate::llm::LlmClient>>>>,
    /// Shared notify slot — telegram.rs sets this per-request with chat_id context
    notify_slot: Arc<std::sync::Mutex<Option<crate::ritual_runner::NotifyFn>>>,
    /// Shared slot for the ritual registry. Late-bound by Agent::new(). When
    /// present, we invalidate its cache around ritual start so the main
    /// agent's next system-prompt build reflects reality immediately.
    registry_slot: Arc<std::sync::Mutex<Option<Arc<crate::ritual_registry::RitualRegistry>>>>,
}

impl StartRitualTool {
    fn new(
        workspace_root: PathBuf,
        llm_client: Option<Arc<tokio::sync::RwLock<Box<dyn crate::llm::LlmClient>>>>,
        notify_slot: Arc<std::sync::Mutex<Option<crate::ritual_runner::NotifyFn>>>,
        registry_slot: Arc<std::sync::Mutex<Option<Arc<crate::ritual_registry::RitualRegistry>>>>,
    ) -> Self {
        Self { workspace_root, llm_client, notify_slot, registry_slot }
    }

    /// Read the current registry from the slot (cheap clone of Arc).
    fn current_registry(&self) -> Option<Arc<crate::ritual_registry::RitualRegistry>> {
        self.registry_slot.lock().ok().and_then(|g| g.clone())
    }
}

#[async_trait]
impl Tool for StartRitualTool {
    fn name(&self) -> &str {
        "start_ritual"
    }

    fn description(&self) -> &str {
        "Start a V2 development ritual (design → implement → verify pipeline). \
         Use this when the task involves writing or modifying source code. \
         The ritual automatically detects project state and runs appropriate phases. \
         Returns when the ritual completes, fails, or needs intervention."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "Description of the development task to accomplish"
                },
                "workspace": {
                    "type": "string",
                    "description": "Optional workspace/project root directory path. If not provided, extracts from task text or uses default workspace."
                }
            },
            "required": ["task"]
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let task = input["task"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'task' parameter"))?
            .to_string();

        // Determine project root: explicit workspace param > extract from task > default
        let workspace_override = input["workspace"]
            .as_str()
            .map(PathBuf::from)
            .filter(|p| p.is_absolute() && p.exists() && p.is_dir());

        let project_root = if let Some(ref ws) = workspace_override {
            ws.clone()
        } else {
            self.workspace_root.clone()
        };

        // If workspace was provided, append it to task text so extract_target_project_dir can use it
        let task = if let Some(ref ws) = workspace_override {
            format!("{}\nProject location: {}", task, ws.display())
        } else {
            task
        };

        let llm_client = match &self.llm_client {
            Some(c) => c.clone(),
            None => {
                return Ok(ToolResult {
                    output: "No LLM client available for ritual execution.".to_string(),
                    is_error: true,
                });
            }
        };

        // Read notify from shared slot (set by telegram.rs per-request), fallback to log-only
        let notify: crate::ritual_runner::NotifyFn = self.notify_slot
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .unwrap_or_else(|| Arc::new(move |msg: String| {
                tracing::info!(ritual_notify = %msg, "Ritual progress");
                Box::pin(async {})
            }));

        let runner = crate::ritual_runner::RitualRunner::new(
            project_root,
            llm_client,
            notify,
        );

        // Invalidate the registry cache before starting — so the new ritual
        // state file is picked up immediately on the next prompt build.
        let reg = self.current_registry();
        if let Some(ref reg) = reg {
            reg.invalidate();
        }

        let result = runner.start(task).await;

        // Invalidate again after the ritual has transitioned — captures the
        // terminal phase (Done/Escalated/Cancelled/WaitingApproval/…) so the
        // main agent's next prompt reflects reality.
        if let Some(ref reg) = reg {
            reg.invalidate();
        }

        match result {
            Ok(state) => {
                if let Err(e) = runner.save_state(&state) {
                    tracing::error!("Failed to save ritual state: {}", e);
                }

                let phase_name = state.phase.display_name();
                let output = match state.phase {
                    gid_core::ritual::state_machine::RitualPhase::Done => {
                        format!("✅ Ritual completed successfully! Final phase: {}", phase_name)
                    }
                    gid_core::ritual::state_machine::RitualPhase::Escalated => {
                        format!(
                            "⚠️ Ritual escalated at {} phase.\nError: {}\nUse /ritual retry to retry.",
                            phase_name,
                            state.error_context.as_deref().unwrap_or("unknown")
                        )
                    }
                    gid_core::ritual::state_machine::RitualPhase::Cancelled => {
                        "🛑 Ritual was cancelled.".to_string()
                    }
                    gid_core::ritual::state_machine::RitualPhase::WaitingApproval => {
                        let review_target = state.review_target.as_deref().unwrap_or("unknown");
                        format!(
                            "⏸️ Ritual paused — waiting for approval of {} review.\n\
                             Review findings are in `.gid/reviews/`.\n\
                             The user can approve via `/ritual approve all` or `/ritual skip`.\n\
                             You may continue implementing independently if the design is already clear.",
                            review_target
                        )
                    }
                    gid_core::ritual::state_machine::RitualPhase::WaitingClarification => {
                        let questions = state.error_context.as_deref().unwrap_or("Task needs clarification");
                        format!(
                            "⏸️ Ritual paused — task is ambiguous and needs clarification.\n\
                             Questions: {}\n\
                             Ask the user for details, then use `/ritual retry` to resume.",
                            questions
                        )
                    }
                    _ => {
                        let mut msg = format!("Ritual ended in {} phase.", phase_name);
                        if let Some(ref err) = state.error_context {
                            msg.push_str(&format!("\nError: {}", err));
                        }
                        if let Some(ref root) = state.target_root {
                            msg.push_str(&format!("\nTarget root: {}", root));
                        }
                        msg.push_str("\nThis usually means project detection failed or the workspace path is wrong.");
                        msg
                    }
                };

                Ok(ToolResult {
                    output,
                    is_error: !matches!(state.phase, gid_core::ritual::state_machine::RitualPhase::Done),
                })
            }
            Err(e) => Ok(ToolResult {
                output: format!("❌ Ritual failed: {}", e),
                is_error: true,
            }),
        }
    }
}

// ── gid_context: assemble token-budget-aware context for target nodes ──

struct GidContextTool {
    mgr: Arc<GraphManager>,
}

#[async_trait]
impl Tool for GidContextTool {
    fn name(&self) -> &str {
        "gid_context"
    }

    fn description(&self) -> &str {
        "Assemble token-budget-aware context for target nodes. Traverses the graph to collect dependencies, callers, and tests with relevance scoring. Returns structured context that fits within the token budget."
    }

    fn input_schema(&self) -> Value {
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "required": ["targets"],
            "properties": {
                "targets": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "One or more node IDs to assemble context for (files, functions, classes, tasks)"
                },
                "token_budget": {
                    "type": "integer",
                    "description": "Maximum token budget for output (default: 8000). Targets are never truncated; transitive deps are dropped first when over budget."
                },
                "depth": {
                    "type": "integer",
                    "description": "Maximum traversal depth in hops (default: 2). depth=1 returns only direct dependencies."
                },
                "include": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Filter patterns: file globs (e.g. '*.rs') or type filters (e.g. 'type:function'). Empty = include all."
                },
                "exclude": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Node IDs to exclude from results."
                },
                "format": {
                    "type": "string",
                    "enum": ["markdown", "json", "yaml"],
                    "description": "Output format (default: markdown). Markdown is optimized for LLM consumption."
                },
                proj_key: proj_val,
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let targets: Vec<String> = match input["targets"].as_array() {
            Some(arr) => arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            None => return Ok(ToolResult {
                output: "Error: 'targets' is required and must be an array of node IDs".to_string(),
                is_error: true,
            }),
        };

        if targets.is_empty() {
            return Ok(ToolResult {
                output: "Error: 'targets' must contain at least one node ID".to_string(),
                is_error: true,
            });
        }

        let token_budget = input["token_budget"].as_u64().unwrap_or(8000) as usize;
        let depth = input["depth"].as_u64().unwrap_or(2) as u32;

        let format = match input["format"].as_str().unwrap_or("markdown") {
            "markdown" | "md" => ContextOutputFormat::Markdown,
            "json" => ContextOutputFormat::Json,
            "yaml" | "yml" => ContextOutputFormat::Yaml,
            other => return Ok(ToolResult {
                output: format!("Unknown format '{}'. Use: markdown, json, yaml", other),
                is_error: true,
            }),
        };

        let include_patterns = input["include"].as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let exclude_ids = input["exclude"].as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        // Derive project_root from graph path: .gid/graph.yml → parent of .gid/
        let (graph_arc, path) = self.mgr.resolve(&input).await?;
        let graph_path = PathBuf::from(path.as_str());
        let project_root = graph_path.parent()
            .and_then(|gid_dir| gid_dir.parent())
            .map(|p| p.to_path_buf());

        let query = ContextQuery {
            targets,
            token_budget,
            depth,
            filters: ContextFilters {
                include_patterns,
                exclude_ids,
                modified_after: None,
            },
            format,
            project_root,
        };

        let graph = graph_arc.read().await;
        match assemble_context(&graph, &query) {
            Ok(result) => {
                let formatted = format_context(&result, query.format);
                Ok(ToolResult {
                    output: format!(
                        "📊 Context: {}/{} nodes, {}/{} tokens, {}ms\n---\n{}",
                        result.stats.nodes_included,
                        result.stats.nodes_visited,
                        result.stats.budget_used,
                        result.stats.budget_total,
                        result.stats.elapsed_ms,
                        formatted
                    ),
                    is_error: false,
                })
            }
            Err(e) => Ok(ToolResult {
                output: format!("Context assembly failed: {}", e),
                is_error: true,
            }),
        }
    }
}

// ── gid_task_context: assemble implementation context for a task node ──

struct GidTaskContextTool {
    mgr: Arc<GraphManager>,
}

#[async_trait]
impl Tool for GidTaskContextTool {
    fn name(&self) -> &str {
        "gid_task_context"
    }

    fn description(&self) -> &str {
        "Assemble implementation context for a task node. Resolves design_ref to extract the relevant design section, maps satisfies GOALs to requirement text, collects guards and dependency interfaces. Returns everything a developer needs to implement the task."
    }

    fn input_schema(&self) -> Value {
        let (proj_key, proj_val) = gid_project_property();
        serde_json::json!({
            "type": "object",
            "required": ["task_id"],
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "Task node ID in the graph"
                },
                proj_key: proj_val,
            }
        })
    }

    async fn execute(&self, input: Value) -> anyhow::Result<ToolResult> {
        let task_id = match input["task_id"].as_str() {
            Some(id) => id,
            None => return Ok(ToolResult {
                output: "Error: 'task_id' is required".to_string(),
                is_error: true,
            }),
        };

        // Derive gid_root from graph path: .gid/graph.yml → .gid/
        let (graph_arc, path) = self.mgr.resolve(&input).await?;
        let graph_path = PathBuf::from(path.as_str());
        let gid_root = graph_path.parent()
            .unwrap_or(std::path::Path::new(".gid"));

        let graph = graph_arc.read().await;
        match assemble_task_context(&graph, task_id, gid_root) {
            Ok(context) => {
                let prompt = context.render_prompt();
                Ok(ToolResult {
                    output: prompt,
                    is_error: false,
                })
            }
            Err(e) => Ok(ToolResult {
                output: format!("Task context assembly failed for '{}': {}", task_id, e),
                is_error: true,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_find_exact() {
        let content = "fn main() {\n    println!(\"hello\");\n}\n";
        let target = "fn main() {\n    println!(\"hello\");\n}";
        let result = EditFileTool::fuzzy_find(content, target);
        assert!(result.is_some(), "Exact match should work");
    }

    #[test]
    fn test_fuzzy_find_whitespace_diff() {
        let content = "fn main() {\n    println!(\"hello\");\n    let x = 1;\n}\n";
        // Target has different indentation (2 spaces vs 4)
        let target = "fn main() {\n  println!(\"hello\");\n  let x = 1;\n}";
        let result = EditFileTool::fuzzy_find(content, target);
        assert!(result.is_some(), "Whitespace-normalized match should work");
        // The returned slice should be the ACTUAL content from the file
        let matched = result.unwrap();
        assert!(matched.contains("    println!"), "Should return original indentation");
    }

    #[test]
    fn test_fuzzy_find_trailing_spaces() {
        let content = "let x = 1;  \nlet y = 2;\n";
        let target = "let x = 1;\nlet y = 2;";
        let result = EditFileTool::fuzzy_find(content, target);
        assert!(result.is_some(), "Trailing space difference should match");
    }

    #[test]
    fn test_fuzzy_find_no_match() {
        let content = "fn main() {\n    println!(\"hello\");\n}\n";
        let target = "fn totally_different() {\n    something_else();\n}";
        let result = EditFileTool::fuzzy_find(content, target);
        assert!(result.is_none(), "Unrelated content should not match");
    }

    #[test]
    fn test_fuzzy_find_single_line_normalized() {
        let content = "    let   x   =   1;\n";
        let target = "let x = 1;";
        let result = EditFileTool::fuzzy_find(content, target);
        assert!(result.is_some(), "Single-line whitespace normalization should match");
    }

    #[test]
    fn test_fuzzy_find_partial_match_below_threshold() {
        // 5 lines, only 2 match = 40% < 80% threshold
        let content = "line1\nline2\nline3\nline4\nline5\n";
        let target = "line1\nXXXX\nXXXX\nXXXX\nline5";
        let result = EditFileTool::fuzzy_find(content, target);
        assert!(result.is_none(), "40% match should be below 80% threshold");
    }

    #[tokio::test]
    async fn test_edit_file_exact_match() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "hello world\n").unwrap();

        let tool = EditFileTool::new(dir.path().to_str().unwrap());
        let result = tool.execute(serde_json::json!({
            "path": file.to_str().unwrap(),
            "old_string": "hello",
            "new_string": "goodbye"
        })).await.unwrap();

        assert!(!result.is_error, "Should succeed: {}", result.output);
        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "goodbye world\n");
    }

    #[tokio::test]
    async fn test_edit_file_multi_edit() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "aaa\nbbb\nccc\n").unwrap();

        let tool = EditFileTool::new(dir.path().to_str().unwrap());
        let result = tool.execute(serde_json::json!({
            "path": file.to_str().unwrap(),
            "edits": [
                {"old_string": "aaa", "new_string": "AAA"},
                {"old_string": "ccc", "new_string": "CCC"}
            ]
        })).await.unwrap();

        assert!(!result.is_error, "Should succeed: {}", result.output);
        assert!(result.output.contains("2 edits applied"));
        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "AAA\nbbb\nCCC\n");
    }

    #[tokio::test]
    async fn test_edit_file_multi_edit_atomic_failure() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "aaa\nbbb\n").unwrap();

        let tool = EditFileTool::new(dir.path().to_str().unwrap());
        let result = tool.execute(serde_json::json!({
            "path": file.to_str().unwrap(),
            "edits": [
                {"old_string": "aaa", "new_string": "AAA"},
                {"old_string": "zzz", "new_string": "ZZZ"}  // This doesn't exist
            ]
        })).await.unwrap();

        assert!(result.is_error, "Should fail on missing second edit");
        // File should be unchanged (atomic — second edit failed but first was applied to in-memory buffer only)
        // Wait — actually our implementation applies sequentially to in-memory buffer then writes.
        // The second edit fails and we return error WITHOUT writing. Let me verify...
        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "aaa\nbbb\n", "File should be unchanged on failure");
    }

    #[tokio::test]
    async fn test_edit_file_fuzzy_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.rs");
        std::fs::write(&file, "fn main() {\n    println!(\"hello\");\n}\n").unwrap();

        let tool = EditFileTool::new(dir.path().to_str().unwrap());
        // old_string has 2-space indent, file has 4-space
        let result = tool.execute(serde_json::json!({
            "path": file.to_str().unwrap(),
            "old_string": "fn main() {\n  println!(\"hello\");\n}",
            "new_string": "fn main() {\n    println!(\"goodbye\");\n}"
        })).await.unwrap();

        assert!(!result.is_error, "Should succeed with fuzzy: {}", result.output);
        assert!(result.output.contains("fuzzy match"), "Should indicate fuzzy was used");
        let content = std::fs::read_to_string(&file).unwrap();
        assert!(content.contains("goodbye"), "File should be edited");
    }
}
