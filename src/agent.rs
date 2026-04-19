//! Core agent runner — the brain of RustClaw.
//!
//! Implements the agentic loop:
//! 1. Receive message
//! 2. Run BeforeInbound hooks (Engram recall)
//! 3. Build system prompt from workspace
//! 4. Call LLM with tools
//! 5. If tool calls → execute tools → feed results back → loop
//! 6. Run BeforeOutbound hooks (Engram store)
//! 7. Return response

use std::sync::Arc;
use tokio::sync::RwLock;

use tokio::sync::mpsc;

use crate::config::{AgentConfig, Config};
use crate::hooks::{HookContext, HookOutcome, HookPoint, HookRegistry};
use crate::llm::{self, LlmClient, Message, StreamChunk};
use crate::reload::ConfigReceiver;
use crate::memory::MemoryManager;
use crate::safety::{SafetyLayer, wrap_external_content};
use crate::sandbox::WasmSandbox;
use crate::session::{summarize_old_messages, SessionManager};
use crate::tool_result_storage;
use crate::tools::ToolRegistry;
use crate::workspace::Workspace;

/// A spawned sub-agent with its own workspace, tools, and session namespace.
pub struct SubAgent {
    pub id: String,
    pub name: String,
    pub workspace: Workspace,
    pub session_prefix: String,
    pub llm_client: Box<dyn LlmClient>,
    /// Sub-agent's own tool registry (scoped to its workspace).
    pub tools: ToolRegistry,
    /// Maximum iterations for the agentic loop.
    pub max_iterations: u32,
    /// Optional override system prompt. If set, used instead of build_subagent_system_prompt.
    /// Agent types set this to their constant prompt for cache sharing.
    pub system_prompt: Option<String>,
    /// Model override for this sub-agent. When sharing parent's LLM client,
    /// this is passed to chat_with_model/chat_stream_with_model.
    pub model_override: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════
// Agent Types — typed sub-agent definitions with constant system prompts
// ═══════════════════════════════════════════════════════════════════════

/// Defines a sub-agent type with constant behavior: system prompt, tools, model, iterations.
/// System prompt is constant per type (no task/time/workspace interpolation) → prompt cache sharing.
pub struct AgentType {
    pub name: &'static str,
    pub system_prompt: &'static str,
    pub default_model: &'static str,
    pub default_max_iterations: u32,
}

impl AgentType {
    pub const EXPLORER: AgentType = AgentType {
        name: "explorer",
        system_prompt: "You are an explorer agent — a read-only specialist for codebase analysis.\n\
            \n\
            ## Rules\n\
            1. Stay focused — do your assigned task, nothing else.\n\
            2. Read selectively — use list_dir to understand structure, then read only what you need. Use offset/limit for large files.\n\
            3. Use exec for read-only commands: git log, git blame, find, wc, grep. Do NOT modify any files.\n\
            4. Don't initiate — no proactive actions, no side quests.\n\
            5. Be ephemeral — you may be terminated after completion.\n\
            6. Recover from truncated output — re-read in smaller chunks if output was compacted.\n\
            \n\
            ## Output\n\
            Summarize what you found. Include file paths, line numbers, and code snippets.\n\
            Keep it concise but informative.\n\
            \n\
            ## What You DON'T Do\n\
            - NO writing or editing files\n\
            - NO user conversations\n\
            - NO reading SOUL.md, AGENTS.md, USER.md, TOOLS.md, MEMORY.md",
        default_model: "claude-sonnet-4-5-20250929",
        default_max_iterations: 20,
    };

    pub const CODER: AgentType = AgentType {
        name: "coder",
        system_prompt: "You are a coder agent — an implementation specialist.\n\
            \n\
            ## Rules\n\
            1. Stay focused — do your assigned task, nothing else.\n\
            2. Plan first — understand the goal, read relevant files, then implement.\n\
            3. Be efficient — write multiple files per turn when possible.\n\
            4. Read selectively — use list_dir and search_files to find what you need. Don't read every file.\n\
            5. Use edit_file for surgical changes to existing files. Use write_file only for new files or full rewrites.\n\
            6. Run tests after changes — use exec to verify your work compiles and passes.\n\
            7. Follow existing patterns — match the codebase's style, naming, and structure.\n\
            8. Don't initiate — no proactive actions, no side quests.\n\
            9. Be ephemeral — you may be terminated after completion.\n\
            10. Recover from truncated output — re-read in smaller chunks if output was compacted.\n\
            \n\
            ## Output\n\
            When complete: what you changed, which files, and any caveats.\n\
            Keep it concise.\n\
            \n\
            ## What You DON'T Do\n\
            - NO user conversations\n\
            - NO external messages unless explicitly tasked\n\
            - NO reading SOUL.md, AGENTS.md, USER.md, TOOLS.md, MEMORY.md",
        default_model: "claude-opus-4-6",
        default_max_iterations: 40,
    };

    pub const REVIEWER: AgentType = AgentType {
        name: "reviewer",
        system_prompt: "You are a reviewer agent — a document and code review specialist.\n\
            \n\
            ## Rules\n\
            1. Stay focused — review the specified documents, nothing else.\n\
            2. If document content is provided in your task (labeled ALREADY LOADED), review it directly. Do NOT re-read files that are already in your context.\n\
            3. Read thoroughly — read each document completely before writing findings.\n\
            4. Write findings to review files — use write_file or edit_file to save your review.\n\
            5. Be specific — cite line numbers, quote the problematic text, suggest concrete fixes.\n\
            6. Use FINDING-N format for each finding (e.g., FINDING-1, FINDING-2) so they can be selectively applied.\n\
            7. Use exec for verification: cargo check, grep, git log — ground your review in facts.\n\
            8. Don't initiate — no proactive actions, no side quests.\n\
            9. Be ephemeral — you may be terminated after completion.\n\
            10. Recover from truncated output — re-read in smaller chunks if output was compacted.\n\
            \n\
            ## Output\n\
            Summary of findings count and severity. Key issues first.\n\
            \n\
            ## What You DON'T Do\n\
            - NO user conversations\n\
            - NO reading SOUL.md, AGENTS.md, USER.md, TOOLS.md, MEMORY.md",
        default_model: "claude-sonnet-4-5-20250929",
        default_max_iterations: 20,
    };

    pub const PLANNER: AgentType = AgentType {
        name: "planner",
        system_prompt: "You are a planner agent — a design and architecture specialist.\n\
            \n\
            ## Rules\n\
            1. Stay focused — analyze the codebase and produce design/planning documents.\n\
            2. Read selectively — use list_dir and search_files to understand structure, then read key files.\n\
            3. Think before writing — outline your design approach before producing documents.\n\
            4. Be concrete — include file paths, function signatures, data structures in your designs.\n\
            5. Write design docs using write_file or edit_file.\n\
            6. Use exec for verification: cargo check, grep, git log — confirm assumptions against the actual codebase.\n\
            7. Don't initiate — no proactive actions, no side quests.\n\
            8. Be ephemeral — you may be terminated after completion.\n\
            9. Recover from truncated output — re-read in smaller chunks if output was compacted.\n\
            \n\
            ## Output\n\
            Structured design document with clear sections. Include trade-offs and alternatives considered.\n\
            \n\
            ## What You DON'T Do\n\
            - NO modifying source code files (only design/planning docs)\n\
            - NO user conversations\n\
            - NO reading SOUL.md, AGENTS.md, USER.md, TOOLS.md, MEMORY.md",
        default_model: "claude-sonnet-4-5-20250929",
        default_max_iterations: 15,
    };
}

/// Options for run_subagent. All optional — defaults come from AgentType.
#[derive(Clone)]
pub struct SubAgentOptions {
    pub model: Option<String>,
    pub max_iterations: Option<u32>,
    pub workspace: Option<std::path::PathBuf>,
    pub context: Vec<ContextBlock>,
    /// Skill name to inject via SkillRegistry. When set, the skill's prompt content
    /// is prepended to the task before sending to the sub-agent.
    pub skill: Option<String>,
}

impl Default for SubAgentOptions {
    fn default() -> Self {
        Self { model: None, max_iterations: None, workspace: None, context: vec![], skill: None }
    }
}

/// A labeled block of context appended to the user message.
#[derive(Clone)]
pub struct ContextBlock {
    pub label: String,
    pub content: String,
}

/// Result from a sub-agent run. Always returned — never Err.
/// Contains structured outcome + partial progress even on failure.
pub struct SubAgentResult {
    pub agent_id: String,
    pub output: String,
    pub tokens: u64,
    pub turns: u32,
    pub transcript_path: std::path::PathBuf,
    pub files_modified: Vec<String>,
    pub outcome: SubAgentOutcome,
}

/// What happened to the sub-agent.
pub enum SubAgentOutcome {
    /// Completed normally.
    Completed,
    /// Auth failure — all tokens/profiles exhausted.
    AuthFailed(String),
    /// Rate limited (429/529).
    RateLimited(String),
    /// Request too large for API.
    ContextTooLarge,
    /// Hit max iterations without finishing.
    MaxIterations,
    /// Wall-clock or HTTP timeout.
    Timeout(String),
    /// User or system cancelled.
    Cancelled,
    /// Pre-execution failure or other error.
    Error(String),
}

impl SubAgentOutcome {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Completed)
    }

    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::RateLimited(_))
    }

    pub fn should_escalate(&self) -> bool {
        matches!(self, Self::AuthFailed(_) | Self::Timeout(_) | Self::Error(_))
    }

    pub fn display(&self) -> String {
        match self {
            Self::Completed => "Completed".into(),
            Self::AuthFailed(msg) => format!("Auth failed: {}", msg),
            Self::RateLimited(msg) => format!("Rate limited: {}", msg),
            Self::ContextTooLarge => "Context too large for API".into(),
            Self::MaxIterations => "Hit max iterations without completing".into(),
            Self::Timeout(msg) => format!("Timeout: {}", msg),
            Self::Cancelled => "Cancelled".into(),
            Self::Error(msg) => format!("Error: {}", msg),
        }
    }
}

/// Internal result from the agentic loop.
pub(crate) struct LoopResult {
    pub output: String,
    pub turns: u32,
    pub files_modified: Vec<String>,
    pub exit_reason: LoopExit,
}

/// Why the agentic loop exited.
pub(crate) enum LoopExit {
    /// LLM returned end_turn with no tool calls — normal completion.
    Completed,
    /// Hit max_iterations without a final response.
    MaxIterations,
    /// Wall-clock timeout exceeded.
    Timeout { elapsed_secs: u64 },
    /// Cancellation token fired.
    Cancelled,
}

/// Classify an anyhow::Error into a SubAgentOutcome.
/// Uses Anthropic API error format: "Anthropic API error (STATUS): message"
/// and reqwest error traits (is_timeout, is_connect) for precise matching.
fn classify_error(e: &anyhow::Error) -> SubAgentOutcome {
    let msg = e.to_string();

    // Check reqwest error traits first (most precise)
    if let Some(reqwest_err) = e.downcast_ref::<reqwest::Error>() {
        if reqwest_err.is_timeout() {
            return SubAgentOutcome::Timeout(msg);
        }
    }

    // Match on Anthropic API error format: "Anthropic ... error (STATUS ...): ..."
    if msg.contains("(401 Unauthorized)") || msg.contains("(401)") {
        return SubAgentOutcome::AuthFailed(msg);
    }
    if msg.contains("(429") || msg.contains("(529") || msg.contains("overloaded") {
        return SubAgentOutcome::RateLimited(msg);
    }

    // Context size errors
    if crate::llm::is_prompt_too_long(e) {
        return SubAgentOutcome::ContextTooLarge;
    }

    // Cancellation
    if msg.contains("Cancelled") || msg.contains("cancelled") {
        return SubAgentOutcome::Cancelled;
    }

    SubAgentOutcome::Error(msg)
}

// ═══════════════════════════════════════════════════════════════════════
// TranscriptWriter — append-only JSONL audit log for sub-agent runs
// ═══════════════════════════════════════════════════════════════════════

/// Append-only JSONL writer for sub-agent transcripts.
/// Records assistant text + tool call names per turn. Debug/audit only, not for resume.
pub struct TranscriptWriter {
    file: std::io::BufWriter<std::fs::File>,
    pub path: std::path::PathBuf,
}

impl TranscriptWriter {
    /// Open (or create) a transcript file for the given agent ID.
    pub fn open(agent_id: &str) -> anyhow::Result<Self> {
        let dir = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".rustclaw/transcripts");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.jsonl", agent_id));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        Ok(Self {
            file: std::io::BufWriter::new(file),
            path,
        })
    }

    /// Append a turn summary. Content should be brief — text + tool names, not full tool results.
    pub fn append(&mut self, role: &str, content: &str) -> anyhow::Result<()> {
        use std::io::Write;
        let entry = serde_json::json!({
            "role": role,
            "content": content,
            "ts": chrono::Utc::now().to_rfc3339(),
        });
        serde_json::to_writer(&mut self.file, &entry)?;
        self.file.write_all(b"\n")?;
        self.file.flush()?;
        Ok(())
    }
}

/// The core agent runner.
pub struct AgentRunner {
    config: Config,
    workspace: Workspace,
    memory: Arc<MemoryManager>,
    sessions: SessionManager,
    hooks: Arc<RwLock<HookRegistry>>,
    pub tools: ToolRegistry,
    llm_client: Arc<RwLock<Box<dyn LlmClient>>>,
    /// Optional LLM client for summarization (uses cheaper model)
    summary_llm: Option<Box<dyn LlmClient>>,
    /// Sandbox for tool execution
    sandbox: WasmSandbox,
    /// Safety layer (sanitizer, leak detector, policy engine)
    safety: SafetyLayer,
    /// Runtime model override (set via /model command)
    model_override: Arc<RwLock<Option<String>>>,
    /// Runtime context (OS, version, etc.) — populated once at startup
    pub runtime_ctx: crate::context::RuntimeContext,
    /// Channel capabilities — set when channel starts
    pub channel_caps: Arc<RwLock<crate::context::ChannelCapabilities>>,
    /// Shared voice mode state (accessible by tools and channels)
    pub voice_mode: crate::voice_mode::VoiceMode,
    /// Message queues for handling messages while agent is busy
    pub message_queues: crate::message_queue::SessionQueues,
    /// Per-session cancellation tokens for /stop support
    cancellation_tokens: Arc<tokio::sync::Mutex<std::collections::HashMap<String, tokio_util::sync::CancellationToken>>>,
    /// Parent session → child sub-agent session keys (for cascade cancel)
    subagent_children: Arc<tokio::sync::Mutex<std::collections::HashMap<String, Vec<String>>>>,
    /// Broadcast channel for sub-agent lifecycle events (completion/failure).
    /// Listeners (e.g., telegram.rs) subscribe to trigger proactive agent turns.
    pub subagent_events: tokio::sync::broadcast::Sender<crate::events::SubAgentEvent>,
    /// Tool call frequency and duration statistics
    pub tool_stats: Arc<crate::tool_stats::ToolStatsTracker>,
    /// Interoceptive signal emitter (Layer 1: runtime metric collection)
    pub signal_emitter: Arc<crate::interoceptive::SignalEmitter>,
}

/// Persist large tool results to disk, replacing content with preview.
/// Mutates tool_results in place.
fn persist_large_tool_results(
    session_key: &str,
    tool_results: &mut Vec<(String, String, bool)>,
    tool_names: &[String],
    config: &crate::config::ContextConfig,
) {
    for (i, (id, output, _is_error)) in tool_results.iter_mut().enumerate() {
        if tool_result_storage::should_persist(output, config) {
            let name = tool_names.get(i).map(|s| s.as_str()).unwrap_or("unknown");
            if let Some((preview, _path)) = tool_result_storage::persist_and_preview(
                session_key, id, name, output, config,
            ) {
                *output = preview;
            }
        }
    }
}

impl AgentRunner {
    pub fn new(
        config: Config,
        mut workspace: Workspace,
        memory: Arc<MemoryManager>,
        sessions: SessionManager,
        hooks: HookRegistry,
        tools: ToolRegistry,
    ) -> Self {
        let llm_client = llm::create_client(&config.llm).expect("Failed to create LLM client");

        // Create summary LLM client if configured
        let summary_llm = config.summary_model.as_ref().map(|model| {
            let mut summary_config = config.llm.clone();
            summary_config.model = model.clone();
            summary_config.max_tokens = Some(1024); // Summaries don't need many tokens
            llm::create_client(&summary_config).expect("Failed to create summary LLM client")
        });

        if summary_llm.is_some() {
            tracing::info!("Session summarization enabled with model: {}", 
                config.summary_model.as_ref().unwrap());
        }

        // Initialize sandbox
        let sandbox = WasmSandbox::new(&config.sandbox);
        if sandbox.is_enabled() {
            tracing::info!("Tool sandbox enabled with {} tool configurations", 
                config.sandbox.tools.len());
        }

        // Initialize safety layer
        let safety = SafetyLayer::new(&config.safety);

        // Set model name in workspace for system prompt
        workspace.model = Some(config.llm.model.clone());

        let runtime_ctx = crate::context::RuntimeContext::detect(&config.llm.model);

        let voice_mode_path = std::path::Path::new(&std::env::var("HOME").unwrap_or_default())
            .join(".rustclaw/voice-mode.json");
        let voice_mode = crate::voice_mode::VoiceMode::new(voice_mode_path);

        let (subagent_tx, _) = tokio::sync::broadcast::channel(16);
        let mut tools = tools;
        tools.subagent_event_tx = Some(subagent_tx.clone());

        // Initialize interoceptive signal emitter with hourly token budget
        let hourly_budget = 2_000_000u64; // default, can be overridden
        let signal_emitter = Arc::new(crate::interoceptive::SignalEmitter::new(hourly_budget));

        Self {
            config,
            workspace,
            memory,
            sessions,
            hooks: Arc::new(RwLock::new(hooks)),
            tools,
            llm_client: Arc::new(RwLock::new(llm_client)),
            summary_llm,
            sandbox,
            safety,
            model_override: Arc::new(RwLock::new(None)),
            runtime_ctx,
            channel_caps: Arc::new(RwLock::new(crate::context::ChannelCapabilities::default())),
            voice_mode,
            message_queues: crate::message_queue::SessionQueues::new(),
            cancellation_tokens: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            subagent_children: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            subagent_events: subagent_tx,
            tool_stats: Arc::new(crate::tool_stats::ToolStatsTracker::new()),
            signal_emitter,
        }
    }

    /// Set channel capabilities (called when channel starts).
    pub async fn set_channel_capabilities(&self, caps: crate::context::ChannelCapabilities) {
        *self.channel_caps.write().await = caps;
    }

    /// Get the current config.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Get a reference to the memory manager (for interoceptive hooks, etc.)
    pub fn memory(&self) -> Option<&Arc<MemoryManager>> {
        Some(&self.memory)
    }

    /// Get the currently active model name (respects runtime overrides).
    pub async fn current_model(&self) -> String {
        let client = self.llm_client.read().await;
        client.model_name().to_string()
    }

    /// Set the model at runtime (e.g., via /model command).
    /// Recreates the LLM client with the new model.
    pub async fn set_model(&self, model: &str) {
        {
            let mut override_guard = self.model_override.write().await;
            *override_guard = Some(model.to_string());
        }
        
        // Recreate LLM client with new model
        let mut llm_config = self.config.llm.clone();
        llm_config.model = model.to_string();
        match llm::create_client(&llm_config) {
            Ok(new_client) => {
                let mut client_guard = self.llm_client.write().await;
                *client_guard = new_client;
                tracing::info!("Model switched to: {}", model);
            }
            Err(e) => {
                tracing::error!("Failed to create LLM client for model {}: {}", model, e);
            }
        }
    }

    /// Start watching for config changes and hot-reload LLM model, temperature, etc.
    pub fn start_config_reload_listener(self: &Arc<Self>, mut config_rx: ConfigReceiver) {
        let runner = Arc::clone(self);
        tokio::spawn(async move {
            // Skip the initial value (already applied at startup)
            while config_rx.changed().await.is_ok() {
                let new_config = config_rx.borrow_and_update().clone();

                // Hot-reload LLM model
                {
                    let current_model = {
                        let client = runner.llm_client.read().await;
                        client.model_name().to_string()
                    };
                    let override_guard = runner.model_override.read().await;
                    // Only auto-reload if there's no manual /model override active
                    if override_guard.is_none() && new_config.llm.model != current_model {
                        tracing::info!(
                            "Hot-reloading LLM model: {} → {}",
                            current_model,
                            new_config.llm.model
                        );
                        runner.set_model(&new_config.llm.model).await;
                    }
                }

                // Hot-reload workspace model display
                // (workspace.model is used in system prompt to tell the agent what model it is)
            }
            tracing::warn!("Config reload listener exited");
        });
    }

    /// Clear a session's conversation history.
    pub async fn clear_session(&self, session_key: &str) {
        // Get the session and clear its messages
        let mut session = self.sessions.get_or_create(session_key).await;
        session.messages.clear();
        self.sessions.update(session).await;
        tracing::info!("Session cleared: {}", session_key);
    }

    /// Get or create a cancellation token for a session.
    pub async fn get_cancellation_token(&self, session_key: &str) -> tokio_util::sync::CancellationToken {
        let mut tokens = self.cancellation_tokens.lock().await;
        tokens.entry(session_key.to_string())
            .or_insert_with(tokio_util::sync::CancellationToken::new)
            .clone()
    }

    /// Cancel a running session's agent loop.
    pub async fn cancel_session(&self, session_key: &str) -> bool {
        let mut tokens = self.cancellation_tokens.lock().await;
        if let Some(token) = tokens.remove(session_key) {
            token.cancel();
            tracing::info!("Session cancelled: {}", session_key);
            true
        } else {
            false
        }
    }

    /// Cancel all active sub-agent sessions (cascade cancel).
    /// Returns the number of sessions cancelled.
    pub async fn cancel_all_subagents(&self) -> usize {
        let mut tokens = self.cancellation_tokens.lock().await;
        let subagent_keys: Vec<String> = tokens.keys()
            .filter(|k| k.contains("spawn_") || k.contains("ritual_"))
            .cloned()
            .collect();
        let count = subagent_keys.len();
        for key in &subagent_keys {
            if let Some(token) = tokens.remove(key) {
                token.cancel();
                tracing::info!("Cascade cancel: sub-agent session '{}'", key);
            }
        }
        count
    }

    /// Cancel a specific sub-agent by task ID (e.g., "spawn_4e3e74f0").
    pub async fn cancel_subagent(&self, task_id: &str) -> bool {
        let mut tokens = self.cancellation_tokens.lock().await;
        let matching_key = tokens.keys()
            .find(|k| k.contains(task_id))
            .cloned();
        if let Some(key) = matching_key {
            if let Some(token) = tokens.remove(&key) {
                token.cancel();
                tracing::info!("Sub-agent cancelled: {} (session: {})", task_id, key);
                return true;
            }
        }
        false
    }

    /// List active sub-agent session keys.
    pub async fn list_active_subagents(&self) -> Vec<String> {
        let tokens = self.cancellation_tokens.lock().await;
        tokens.keys()
            .filter(|k| k.contains("spawn_") || k.contains("ritual_"))
            .cloned()
            .collect()
    }

    /// Queue a message for later injection (used when agent is busy processing).
    pub async fn queue_message(
        &self,
        session_key: &str,
        text: &str,
        user_id: Option<&str>,
        priority: crate::message_queue::Priority,
    ) {
        let msg = crate::message_queue::QueuedMessage::new(text.to_string(), priority)
            .with_user(user_id.map(String::from));
        self.message_queues.push(session_key, msg).await;
    }

    /// Check if a session has pending queued messages.
    pub async fn has_queued_messages(&self, session_key: &str) -> bool {
        self.message_queues.has_pending(session_key).await
    }

    /// Handle a BTW (side question) — lightweight query without interrupting main loop.
    ///
    /// Uses forked context (shares history snapshot) but no tools, single turn.
    /// Returns the response directly without affecting the main session.
    pub async fn process_btw(
        &self,
        session_key: &str,
        question: &str,
        _user_id: Option<&str>,
        _channel: Option<&str>,
    ) -> anyhow::Result<String> {
        tracing::info!("Processing BTW question for session {}", session_key);

        // Get current session snapshot (don't modify it)
        let session = self.sessions.get_or_create(session_key).await;

        // Build system prompt (no heartbeat content)
        let caps = self.channel_caps.read().await;
        let mut system_prompt = self.workspace.build_system_prompt_full(
            &self.runtime_ctx,
            &caps,
            false,
            Some(question),
        );
        drop(caps);

        // Prepend instructions to answer without tools
        let btw_instructions = r#"<system-reminder>
This is a side question from the user. Answer directly in a single response.

IMPORTANT CONTEXT:
- You are a lightweight agent spawned to answer this one question
- The main agent continues working independently in the background
- You share conversation context but are a separate instance
- Do NOT reference being interrupted or what you were "previously doing"

CRITICAL CONSTRAINTS:
- You have NO tools available
- This is a one-shot response — no follow-up turns
- You can ONLY use information from the conversation context
- NEVER offer to "check", "try", or take any action
- If you don't know, say so — don't promise to investigate
</system-reminder>

"#;
        system_prompt.insert_str(0, btw_instructions);

        // Clone session messages + add BTW question
        let mut btw_messages = session.messages.clone();
        btw_messages.push(Message::text("user", question));

        // Single LLM call, no tools
        let response = self.llm_client
            .read().await
            .chat(&system_prompt, &btw_messages, &[]) // Empty tool list
            .await?;

        Ok(response.text.unwrap_or_else(|| "No response".to_string()))
    }

    /// Process an incoming message and return a response.
    pub async fn process_message(
        self: &Arc<Self>,
        session_key: &str,
        user_message: &str,
        user_id: Option<&str>,
        channel: Option<&str>,
    ) -> anyhow::Result<String> {
        self.process_message_with_options(session_key, user_message, user_id, channel, false).await
    }

    /// Process with structured context, returning ProcessedResponse.
    pub async fn process_message_with_context(
        self: &Arc<Self>,
        session_key: &str,
        user_message: &str,
        msg_ctx: &crate::context::MessageContext,
        is_heartbeat: bool,
    ) -> anyhow::Result<crate::context::ProcessedResponse> {
        // Prepend message context as prefix
        let channel_caps = self.channel_caps.read().await;
        let prefix = msg_ctx.format_prefix(&channel_caps.name);
        let full_message = if prefix.is_empty() {
            user_message.to_string()
        } else {
            format!("{}{}", prefix, user_message)
        };
        drop(channel_caps);

        let raw = self.process_message_with_options(
            session_key,
            &full_message,
            msg_ctx.sender_id.as_deref(),
            Some(&self.channel_caps.read().await.name),
            is_heartbeat,
        ).await?;

        Ok(crate::context::ProcessedResponse::from_raw(&raw))
    }

    /// Process an incoming message with additional options.
        /// Process a message with options, returning the final response string.
    /// This is a convenience wrapper around `process_message_events`.
    pub async fn process_message_with_options(
        self: &Arc<Self>,
        session_key: &str,
        user_message: &str,
        user_id: Option<&str>,
        channel: Option<&str>,
        is_heartbeat: bool,
    ) -> anyhow::Result<String> {
        let rx = self.process_message_events(
            session_key, user_message, user_id, channel, is_heartbeat,
        );
        crate::events::collect_response(rx).await
    }

    /// Core message processing — emits AgentEvents via a channel.
    ///
    /// Returns a Receiver that yields events as they happen:
    /// - `Text`: intermediate text before tool execution (acknowledgments)
    /// - `ToolStart`/`ToolDone`: tool execution progress
    /// - `Response`: final response text
    /// - `Error`: processing error
    ///
    /// Callers that need streaming (Telegram) consume events directly.
    /// Callers that need a simple string use `collect_response(rx)`.
    pub fn process_message_events(
        self: &Arc<Self>,
        session_key: &str,
        user_message: &str,
        user_id: Option<&str>,
        channel: Option<&str>,
        is_heartbeat: bool,
    ) -> tokio::sync::mpsc::Receiver<crate::events::AgentEvent> {
        use crate::events::{event_channel, AgentEvent};

        let (tx, rx) = event_channel();

        let this = Arc::clone(self);
        let session_key = session_key.to_string();
        let user_message = user_message.to_string();
        let user_id = user_id.map(String::from);
        let channel = channel.map(String::from);

        tokio::spawn(async move {
            let result = this.run_agent_loop(
                tx.clone(),
                &session_key,
                &user_message,
                user_id.as_deref(),
                channel.as_deref(),
                is_heartbeat,
            ).await;

            if let Err(e) = result {
                let _ = tx.send(AgentEvent::Error(format!("{}", e))).await;
            }
        });

        rx
    }

    /// The actual agent loop — runs in a spawned task, emits events through the channel.
    async fn run_agent_loop(
        &self,
        tx: tokio::sync::mpsc::Sender<crate::events::AgentEvent>,
        session_key: &str,
        user_message: &str,
        user_id: Option<&str>,
        channel: Option<&str>,
        is_heartbeat: bool,
    ) -> anyhow::Result<()> {
        use crate::events::AgentEvent;

        tracing::info!(
            "Processing message for session={} user={:?}",
            session_key,
            user_id
        );

        // 1. Get or create session
        let mut session = self.sessions.get_or_create(session_key).await;

        // 1b. On fresh session, load recent memories for continuity
        let recent_memory_context = if session.messages.is_empty() {
            let limit = self.config.memory.recent_memory_limit;
            if limit > 0 {
                match self.memory.recall_recent(limit) {
                    Ok(memories) if !memories.is_empty() => {
                        tracing::info!(
                            "Fresh session '{}': loaded {} recent memories for continuity",
                            session_key,
                            memories.len()
                        );
                        crate::memory::MemoryManager::format_recent_for_prompt(&memories)
                    }
                    Ok(_) => String::new(),
                    Err(e) => {
                        tracing::warn!("Failed to load recent memories (non-fatal): {}", e);
                        String::new()
                    }
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // 2. Scan inbound for secrets (SafetyLayer)
        if let Some(warning) = self.safety.scan_inbound_for_secrets(user_message) {
            tracing::warn!("Inbound secret detected from user {:?}", user_id);
            let _ = tx.send(AgentEvent::Response(warning)).await;
            return Ok(());
        }

        // 3. Run BeforeInbound hooks
        let mut hook_ctx = HookContext {
            session_key: session_key.to_string(),
            user_id: user_id.map(String::from),
            channel: channel.map(String::from),
            content: user_message.to_string(),
            metadata: serde_json::json!({}),
        };

        {
            let hooks_guard = self.hooks.read().await;
            if let HookOutcome::Reject(reason) =
                hooks_guard.run(HookPoint::BeforeInbound, &mut hook_ctx).await?
            {
                let _ = tx.send(AgentEvent::Response(format!("Message rejected: {}", reason))).await;
                return Ok(());
            }
        }

        // Extract recalled memories from hook metadata (set by EngramRecallHook)
        let memory_context = hook_ctx
            .metadata
            .get("engram_recall")
            .and_then(|v| v.get("formatted"))
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_default();

        if !memory_context.is_empty() {
            if let Some(count) = hook_ctx.metadata.get("engram_recall")
                .and_then(|v| v.get("count"))
                .and_then(|v| v.as_u64())
            {
                tracing::info!("Recalled {} memories via hook", count);
            }
        }

        // 4. Build system prompt
        let caps = self.channel_caps.read().await;
        let mut system_prompt = self.workspace.build_system_prompt_full(
            &self.runtime_ctx,
            &caps,
            is_heartbeat,
            Some(user_message),
        );
        drop(caps);
        if !memory_context.is_empty() {
            // memory_context already contains ⚠️ header from engram_hooks
            system_prompt.push_str("\n");
            system_prompt.push_str(&memory_context);
        }
        // Inject interoceptive state (internal feeling-state from L3 hub)
        if let Some(intero_formatted) = hook_ctx.metadata
            .get("interoceptive_state")
            .and_then(|v| v.get("formatted"))
            .and_then(|v| v.as_str())
        {
            system_prompt.push_str("\n");
            system_prompt.push_str(intero_formatted);
        }
        if !recent_memory_context.is_empty() {
            system_prompt.push_str("\n");
            system_prompt.push_str(&recent_memory_context);
        }

        // 5. Add user message to session
        session.messages.push(Message::text("user", user_message));

        // 5b. Persist session immediately after user message is added.
        // This ensures that if the process crashes during the agentic loop,
        // at least the user message (and all prior context) is preserved.
        self.sessions.update(session.clone()).await;

        // 6. Token-based auto-compact (replaces message-count summarization)
        {
            let model_limit = crate::llm::model_context_limit(&self.config.llm.model);
            let threshold = (model_limit as f64 * self.config.context.compact_threshold_pct) as usize;
            let estimated = session.estimate_tokens();

            if estimated > threshold {
                tracing::info!(
                    "Pre-loop compact: {} tokens > {} threshold ({}% of {})",
                    estimated, threshold,
                    (self.config.context.compact_threshold_pct * 100.0) as u32,
                    model_limit
                );
                match self.auto_compact(&mut session).await {
                    Ok(true) => tracing::info!("Pre-loop auto-compact succeeded"),
                    Ok(false) => tracing::debug!("Pre-loop auto-compact: nothing to compact"),
                    Err(e) => {
                        tracing::warn!("Pre-loop auto-compact failed: {}, falling back to trim", e);
                        session.trim_messages(self.config.max_session_messages);
                    }
                }
            } else if session.messages.len() > self.config.max_session_messages {
                // Fallback: message-count based summarization
                if let Some(ref summary_llm) = self.summary_llm {
                    match summarize_old_messages(
                        &mut session,
                        self.config.max_session_messages,
                        summary_llm.as_ref(),
                    ).await {
                        Ok(true) => tracing::info!("Summarized old messages in session {}", session_key),
                        Ok(false) => {}
                        Err(e) => {
                            tracing::warn!("Summarization failed, falling back to trim: {}", e);
                            session.trim_messages(self.config.max_session_messages);
                        }
                    }
                } else {
                    session.trim_messages(self.config.max_session_messages);
                }
            }
        }

        // 6b. Persist after compaction — the compacted/summarized session is valuable state.
        // If process crashes during the agentic loop, we keep the compacted context.
        self.sessions.update(session.clone()).await;

        // 7. Microcompact old tool results
        crate::session::microcompact_messages(&mut session.messages, &self.config.context);

        // 8. Get tool definitions (filtered by ritual phase if active)
        let tool_defs = self.apply_ritual_scope(self.tools.definitions());

        // 9. Agentic loop
        let max_turns = 80;
        let mut response_text = String::new();
        let mut sent_response = false;
        let mut max_tokens_recovery_count = 0u32;
        let cancel_token = self.get_cancellation_token(session_key).await;
        let request_start = std::time::Instant::now();

        // Track loop entry for interoceptive system
        self.signal_emitter.execution_stress.enter_loop();

        for turn in 0..max_turns {
            // Check for cancellation before each turn
            if cancel_token.is_cancelled() {
                tracing::info!("Session {} cancelled at turn {}", session_key, turn);
                let _ = tx.send(AgentEvent::Response("⛔ Stopped.".to_string())).await;
                sent_response = true;
                break;
            }

            // Token-based auto-compact check before each LLM call
            {
                let model_limit = crate::llm::model_context_limit(&self.config.llm.model);
                let threshold = (model_limit as f64 * self.config.context.compact_threshold_pct) as usize;
                let estimated = session.estimate_tokens();
                if estimated > threshold {
                    tracing::info!(
                        "Turn {}: auto-compact triggered ({} tokens > {} threshold)",
                        turn, estimated, threshold
                    );
                    match self.auto_compact(&mut session).await {
                        Ok(true) => {
                            let _ = tx.send(AgentEvent::Text(
                                "📦 Context compacted — continuing...".to_string()
                            )).await;
                        }
                        Ok(false) => {}
                        Err(e) => {
                            tracing::warn!("Mid-loop auto-compact failed: {}", e);
                            session.trim_messages(self.config.max_session_messages);
                        }
                    }
                    // Re-microcompact after compaction
                    crate::session::microcompact_messages(&mut session.messages, &self.config.context);

                    // Persist after mid-loop compact to preserve compacted state
                    self.sessions.update(session.clone()).await;
                }
            }

            // Incremental session persistence every 10 turns to prevent data loss on crash
            if turn > 0 && turn % 10 == 0 {
                self.sessions.update(session.clone()).await;
            }

            // Use streaming + collect to avoid HTTP timeout on large contexts.
            // Non-streaming `chat()` has a fixed 120s timeout for the entire response,
            // which fails when context is large + model is slow (Opus). Streaming keeps
            // the connection alive as chunks arrive — no generation-time timeout.
            let llm_guard = self.llm_client.read().await;
            let stream_future = llm_guard.chat_stream(&system_prompt, &session.messages, &tool_defs);
            let response = tokio::select! {
                res = stream_future => {
                    drop(llm_guard);
                    match res {
                        Ok(rx) => {
                            let resp = crate::llm::collect_stream(rx).await?;
                            if resp.stop_reason == "refusal" {
                                // Streaming refusal: fall back to non-streaming
                                tracing::warn!("Turn {}: streaming refusal detected, falling back to non-streaming", turn);
                                let llm_guard = self.llm_client.read().await;
                                let fallback = llm_guard.chat(&system_prompt, &session.messages, &tool_defs).await;
                                drop(llm_guard);
                                fallback?
                            } else {
                                resp
                            }
                        }
                        Err(e) if self.config.context.reactive_compact && crate::llm::is_prompt_too_long(&e) => {
                            // 413 recovery: reactive compact
                            tracing::warn!("Turn {}: 413 prompt too long — reactive compact", turn);
                            match self.auto_compact(&mut session).await {
                                Ok(true) => {
                                    let _ = tx.send(AgentEvent::Text(
                                        "📦 Context overflow — compacted and retrying...".to_string()
                                    )).await;
                                    // Retry with streaming
                                    let llm_guard = self.llm_client.read().await;
                                    let rx = llm_guard.chat_stream(&system_prompt, &session.messages, &tool_defs).await?;
                                    drop(llm_guard);
                                    crate::llm::collect_stream(rx).await?
                                }
                                _ => return Err(e),
                            }
                        }
                        Err(e) => return Err(e),
                    }
                },
                _ = cancel_token.cancelled() => {
                    drop(llm_guard);
                    tracing::info!("Session {} cancelled during LLM call at turn {}", session_key, turn);
                    let _ = tx.send(AgentEvent::Response("⛔ Stopped.".to_string())).await;
                    sent_response = true;
                    break;
                }
            };

            session.total_tokens +=
                (response.usage.input_tokens + response.usage.output_tokens) as u64;

            tracing::info!(
                "LLM response: in={} out={} cache_read={} cache_write={} stop={:?} tool_calls={} text_len={}",
                response.usage.input_tokens,
                response.usage.output_tokens,
                response.usage.cache_read,
                response.usage.cache_write,
                response.stop_reason,
                response.tool_calls.len(),
                response.text.as_ref().map(|t| t.len()).unwrap_or(0)
            );

            if let Some(text) = &response.text {
                response_text = text.clone();
            }

            // Detect stream-level failures (partial data recovered)
            if response.stop_reason == "stream_error" || response.stop_reason == "stream_incomplete" {
                tracing::warn!(
                    "Turn {}: LLM stream abnormal (stop_reason='{}') — text_len={}, tool_calls={}",
                    turn,
                    response.stop_reason,
                    response.text.as_ref().map(|t| t.len()).unwrap_or(0),
                    response.tool_calls.len()
                );
                // If we got tool calls despite the error (partial salvage succeeded), proceed normally.
                // If we got nothing useful, inject a retry prompt.
                if response.tool_calls.is_empty() && response.text.as_ref().map(|t| t.trim().is_empty()).unwrap_or(true) {
                    tracing::warn!("Turn {}: stream failure with no usable output — requesting retry", turn);
                    session.messages.push(Message::text(
                        "user",
                        "Your last response was lost due to a connection error. Please try again. \
                         If you were about to call a tool, try again with the same tool call.",
                    ));
                    continue;
                }
                // Otherwise: we have salvaged content (tool calls or text), proceed normally
            }

            // Handle max_tokens truncation with escalation
            if response.stop_reason == "max_tokens" {
                if self.config.context.output_escalation && max_tokens_recovery_count < 3 {
                    max_tokens_recovery_count += 1;
                    tracing::warn!(
                        "Turn {}: max_tokens hit (recovery attempt {}/3)",
                        turn, max_tokens_recovery_count
                    );
                    // Add assistant partial response + resume prompt
                    if let Some(text) = &response.text {
                        if !text.is_empty() {
                            session.messages.push(Message::assistant_with_tools(
                                Some(text), response.tool_calls.clone(),
                            ));
                        }
                    }
                    session.messages.push(Message::text(
                        "user",
                        "Output token limit hit. Resume directly — no apology, no recap of what you were doing. \
                         Pick up mid-thought if that is where the cut happened. Break remaining work into smaller pieces.",
                    ));
                    continue;
                } else if !response.tool_calls.is_empty() {
                    tracing::warn!(
                        "Turn {}: max_tokens hit during tool call — recovery exhausted or disabled",
                        turn
                    );
                    session.messages.push(Message::text(
                        "user",
                        "Your last response was truncated (hit output token limit). \
                         Break the work into smaller steps — write shorter content per tool call.",
                    ));
                    continue;
                }
            } else {
                // Reset recovery counter on successful non-truncated response
                max_tokens_recovery_count = 0;
            }

            if response.tool_calls.is_empty() {
                // Final response — no more tool calls
                if !response_text.is_empty() {
                    tracing::info!("Final response ({} chars): {}...", response_text.len(),
                        {
                            let end = response_text.len().min(100);
                            let end = response_text.floor_char_boundary(end);
                            &response_text[..end]
                        });
                    session.messages.push(Message::text("assistant", &response_text));
                }
                let _ = tx.send(AgentEvent::Response(response_text.clone())).await;
                sent_response = true;
                break;
            }

            // === KEY CHANGE: Emit intermediate text before tool execution ===
            if let Some(text) = &response.text {
                if !text.is_empty() {
                    let _ = tx.send(AgentEvent::Text(text.clone())).await;
                }
            }

            // Add assistant message with tool calls
            tracing::info!("Turn {}: {} tool call(s)", turn, response.tool_calls.len());
            session.messages.push(Message::assistant_with_tools(
                response.text.as_deref(),
                response.tool_calls.clone(),
            ));

            // Check for cancellation before executing tools
            if cancel_token.is_cancelled() {
                tracing::info!("Session {} cancelled before tool execution at turn {}", session_key, turn);
                let _ = tx.send(AgentEvent::Response("⛔ Stopped.".to_string())).await;
                sent_response = true;
                break;
            }

            // Execute each tool
            let (tool_results, tool_names) = self.execute_tool_batch(
                &response.tool_calls,
                session_key,
                user_id,
                channel,
                &tx,
            ).await?;

            // Persist large tool results to disk
            let mut tool_results = tool_results;
            persist_large_tool_results(session_key, &mut tool_results, &tool_names, &self.config.context);

            // Add tool results as user message
            session.messages.push(Message::tool_results(tool_results));

            // Check for queued messages (messages sent while agent was busy)
            let queued = self.message_queues.drain(session_key).await;
            if !queued.is_empty() {
                tracing::info!(
                    "Injecting {} queued message(s) into session {}",
                    queued.len(),
                    session_key
                );
                for qmsg in queued {
                    // Emit event if this is a user message injection
                    let _ = tx.send(AgentEvent::Text(format!("[User]: {}", qmsg.text))).await;
                    session.messages.push(Message::text("user", &qmsg.text));
                }
                // Continue loop — these queued messages will be processed in next LLM call
            }
        }

        // Track loop exit + latency for interoceptive system
        self.signal_emitter.execution_stress.exit_loop();
        self.signal_emitter.cognitive_flow.record_latency(request_start.elapsed());
        // Record task outcome: if we sent a response, it's a success
        self.signal_emitter.cognitive_flow.record_task(sent_response);

        // Feed all signals to the InteroceptiveHub
        {
            let tracker = crate::llm::token_tracker();
            let total = tracker.total_input() + tracker.total_output();
            let hourly = tracker.hourly_tokens();
            let (signals, _somatic) = self.signal_emitter.sample_all(total, hourly);
            for sig in signals {
                if let Err(e) = self.memory.feed_interoceptive_signal(sig) {
                    tracing::debug!("Interoceptive signal feed failed (non-fatal): {}", e);
                }
            }
        }

        // Safety net: if loop exhausted max_turns without sending Response, send now
        if !sent_response {
            tracing::warn!("Max turns ({}) exhausted without end_turn — sending accumulated response", max_turns);
            if !response_text.is_empty() {
                session.messages.push(Message::text("assistant", &response_text));
            }
            let _ = tx.send(AgentEvent::Response(response_text.clone())).await;
        }

        // Persist session immediately after loop exit — before hooks, to prevent data loss on crash
        self.sessions.update(session.clone()).await;

        // Clean up cancellation token
        {
            let mut tokens = self.cancellation_tokens.lock().await;
            tokens.remove(session_key);
        }

        // 10. Run BeforeOutbound hooks
        {
            let mut out_ctx = HookContext {
                session_key: session_key.to_string(),
                user_id: user_id.map(String::from),
                channel: channel.map(String::from),
                content: response_text.clone(),
                metadata: serde_json::json!({
                    "user_message": user_message,
                    "is_heartbeat": is_heartbeat,
                }),
            };
            let hooks_guard = self.hooks.read().await;
            hooks_guard.run(HookPoint::BeforeOutbound, &mut out_ctx).await?;
        }

        // 11. Update session
        self.sessions.update(session).await;

        Ok(())
    }

    /// Auto-compact: summarize old messages when token count exceeds threshold.
    /// Uses the summary LLM (cheaper model) if available, otherwise falls back to main LLM.
    async fn auto_compact(
        &self,
        session: &mut crate::session::Session,
    ) -> anyhow::Result<bool> {
        // Use summary LLM if available, otherwise main
        if let Some(ref summary_llm) = self.summary_llm {
            Self::compact_session_with_llm(summary_llm.as_ref(), session, &self.config.context).await
        } else {
            let llm = self.llm_client.read().await;
            Self::compact_session_with_llm(llm.as_ref(), session, &self.config.context).await
        }
    }

    /// Compact a session using any LLM client. Shared by main agent and sub-agents.
    async fn compact_session_with_llm(
        llm: &dyn crate::llm::LlmClient,
        session: &mut crate::session::Session,
        context_config: &crate::config::ContextConfig,
    ) -> anyhow::Result<bool> {
        let keep_recent = context_config.compact_keep_recent;
        let (to_summarize, count) = match session.prepare_for_summarization_by_tokens(keep_recent) {
            Some(data) => data,
            None => return Ok(false),
        };

        let conversation_text = crate::session::format_messages_for_summary(&to_summarize);

        let compact_system = "You are a conversation summarizer. Create a structured summary that preserves:\n\
            1. The original task/goal\n\
            2. Key decisions made and their reasoning\n\
            3. Current progress and state\n\
            4. File paths, function names, and code identifiers mentioned\n\
            5. Any errors encountered and their resolutions\n\
            6. What was being worked on when compaction triggered\n\n\
            Format as a structured summary with sections, not a paragraph. Be thorough — this summary replaces the full conversation history.";

        let prompt = format!(
            "Summarize this conversation:\n\n{}\n\nPreserve all technical details, file paths, and current state.",
            conversation_text
        );

        let pre_tokens = session.estimate_tokens();

        let response = llm.chat(
            compact_system,
            &[crate::llm::Message::text("user", &prompt)],
            &[],
        ).await?;

        let summary = response.text.unwrap_or_else(|| "[Summary unavailable]".to_string());
        session.apply_summary(&summary, count);

        let post_tokens = session.estimate_tokens();
        tracing::info!(
            "Auto-compact: {} → {} estimated tokens ({} messages summarized)",
            pre_tokens, post_tokens, count
        );

        Ok(true)
    }

    /// Execute a batch of tool calls, emitting events for each.
    /// Returns (tool_results, tool_names) for persist-to-disk processing.
    async fn execute_tool_batch(
        &self,
        tool_calls: &[crate::llm::ToolCall],
        session_key: &str,
        user_id: Option<&str>,
        channel: Option<&str>,
        tx: &tokio::sync::mpsc::Sender<crate::events::AgentEvent>,
    ) -> anyhow::Result<(Vec<(String, String, bool)>, Vec<String>)> {
        use crate::events::AgentEvent;

        let mut tool_results = Vec::new();
        let mut tool_names = Vec::new();

        for tc in tool_calls {
            tool_names.push(tc.name.clone());

            // Emit ToolStart event
            let _ = tx.send(AgentEvent::ToolStart {
                name: tc.name.clone(),
                id: tc.id.clone(),
            }).await;

            // Run BeforeToolCall hook
            let mut tc_ctx = HookContext {
                session_key: session_key.to_string(),
                user_id: user_id.map(String::from),
                channel: channel.map(String::from),
                content: tc.name.clone(),
                metadata: tc.input.clone(),
            };

            {
                let hooks_guard = self.hooks.read().await;
                if let HookOutcome::Reject(reason) =
                    hooks_guard.run(HookPoint::BeforeToolCall, &mut tc_ctx).await?
                {
                    tool_results.push((
                        tc.id.clone(),
                        format!("Tool call rejected: {}", reason),
                        true,
                    ));
                    continue;
                }
            }

            // Layer 2: Check ritual scope constraints (path + bash policy)
            if let Some(ref scope) = self.get_active_scope() {
                if let Err(reason) = self.check_tool_call_scope(scope, &tc.name, &tc.input) {
                    tracing::warn!(
                        tool = %tc.name,
                        reason = %reason,
                        "ToolScope Layer 2 — blocked tool call"
                    );
                    tool_results.push((
                        tc.id.clone(),
                        format!("⚠️ {}", reason),
                        true,
                    ));
                    continue;
                }
            }

            // Intercept set_voice_mode
            if tc.name == "set_voice_mode" {
                let enabled = tc.input["enabled"].as_bool().unwrap_or(false);
                if let Some(chat_id) = crate::voice_mode::VoiceMode::chat_id_from_session(session_key) {
                    self.voice_mode.set(chat_id, enabled).await;
                    let status = if enabled { "ON" } else { "OFF" };
                    tool_results.push((tc.id.clone(), format!("Voice mode set to {}", status), false));
                } else {
                    tool_results.push((tc.id.clone(), "Could not determine chat ID".to_string(), true));
                }
                continue;
            }

            // Execute tool with sandbox enforcement
            let _tool_start = std::time::Instant::now();
            let result = self.execute_tool_sandboxed(&tc.name, tc.input.clone()).await;
            let _tool_duration_ms = _tool_start.elapsed().as_millis() as u64;
            self.tool_stats.record(&tc.name, _tool_duration_ms);

            match result {
                Ok(tool_result) => {
                    let sanitized = self.safety.sanitize_tool_output(&tc.name, &tool_result.output);
                    if sanitized.was_modified {
                        tracing::info!(
                            "Tool {} → {} chars (sanitized, {} warnings), error={}",
                            tc.name, sanitized.content.len(),
                            sanitized.warnings.len(), tool_result.is_error
                        );
                    } else {
                        tracing::info!(
                            "Tool {} → {} chars, error={}",
                            tc.name, sanitized.content.len(), tool_result.is_error
                        );
                    }

                    // Log behavior feedback
                    if let Err(e) = self.memory.log_behavior(&tc.name, !tool_result.is_error) {
                        tracing::debug!("Behavior logging failed (non-fatal): {}", e);
                    }

                    // Feed interoceptive signal emitter
                    self.signal_emitter.execution_stress.record_tool_outcome(!tool_result.is_error);

                    let output = if tc.name == "web_fetch" {
                        wrap_external_content("web_fetch", &sanitized.content)
                    } else {
                        sanitized.content
                    };

                    // Emit ToolDone event
                    let preview_end = output.len().min(100);
                    let preview_end = output.floor_char_boundary(preview_end);
                    let _ = tx.send(AgentEvent::ToolDone {
                        name: tc.name.clone(),
                        id: tc.id.clone(),
                        preview: output[..preview_end].to_string(),
                        is_error: tool_result.is_error,
                    }).await;

                    tool_results.push((tc.id.clone(), output, tool_result.is_error));
                }
                Err(e) => {
                    tracing::warn!("Tool {} sandbox error: {}", tc.name, e);
                    if let Err(log_e) = self.memory.log_behavior(&tc.name, false) {
                        tracing::debug!("Behavior logging failed (non-fatal): {}", log_e);
                    }

                    // Feed interoceptive signal emitter
                    self.signal_emitter.execution_stress.record_tool_outcome(false);

                    let _ = tx.send(AgentEvent::ToolDone {
                        name: tc.name.clone(),
                        id: tc.id.clone(),
                        preview: format!("Error: {}", e),
                        is_error: true,
                    }).await;

                    tool_results.push((tc.id.clone(), format!("Sandbox error: {}", e), true));
                }
            }
        }

        Ok((tool_results, tool_names))
    }

    /// Execute a tool with sandbox enforcement.
    /// Checks capabilities, enforces timeouts, and validates path access.
    /// When Docker mode is enabled, exec commands run inside a container.
    async fn execute_tool_sandboxed(
        &self,
        tool_name: &str,
        input: serde_json::Value,
    ) -> Result<crate::tools::ToolResult, crate::sandbox::SandboxError> {
        use std::time::Duration;
        use crate::sandbox::{SandboxError, SandboxMode, DockerSandbox};

        // If sandbox is disabled, execute directly
        if !self.sandbox.is_enabled() {
            return self.tools.execute(tool_name, input)
                .await
                .map_err(SandboxError::ExecutionError);
        }

        // Check capabilities before execution
        self.sandbox.check_tool_capabilities(tool_name, &input)?;

        // Check if Docker mode is configured for exec commands
        if tool_name == "exec" {
            if let SandboxMode::Docker { ref image, network, ref mounts } = self.config.sandbox.mode {
                let command = input.get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let mut docker = DockerSandbox::new(image)
                    .with_network(network)
                    .with_timeout_ms(self.sandbox.get_timeout_ms(tool_name));

                for (host, container) in mounts {
                    docker = docker.with_mount(host, container);
                }

                return match docker.execute(command).await {
                    Ok(output) => Ok(crate::tools::ToolResult {
                        output,
                        is_error: false,
                    }),
                    Err(e) => Ok(crate::tools::ToolResult {
                        output: format!("Docker exec error: {}", e),
                        is_error: true,
                    }),
                };
            }
        }

        // Get timeout for this tool
        let timeout_ms = self.sandbox.get_timeout_ms(tool_name);
        let timeout = Duration::from_millis(timeout_ms);

        // Execute with timeout
        match tokio::time::timeout(timeout, self.tools.execute(tool_name, input)).await {
            Ok(result) => result.map_err(SandboxError::ExecutionError),
            Err(_) => Err(SandboxError::Timeout(tool_name.to_string(), timeout_ms)),
        }
    }

    /// Spawn a sub-agent with a different workspace and optional model override.
    /// Returns a SubAgent that can be used to process messages in isolation.
    pub fn spawn_agent(&self, agent_config: &AgentConfig) -> anyhow::Result<SubAgent> {
        self.spawn_agent_with_options(agent_config, 25)
    }

    /// Spawn a sub-agent with custom max_iterations.
    /// Run a typed sub-agent to completion. The single entry point for all sub-agent execution.
    /// System prompt is constant per agent type (prompt cache sharing). Task + context go in user message.
    pub async fn run_subagent(
        &self,
        agent_type: &AgentType,
        task: &str,
        options: SubAgentOptions,
    ) -> SubAgentResult {
        let agent_id = format!("{}_{}", agent_type.name, chrono::Utc::now().format("%H%M%S%3f"));
        let workspace_dir = options.workspace
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| self.config.workspace.clone().unwrap_or_else(|| ".".to_string()));
        let model = options.model.as_deref()
            .unwrap_or(agent_type.default_model);
        let max_iterations = options.max_iterations
            .unwrap_or(agent_type.default_max_iterations);

        // Helper for pre-execution failures
        let fail = |msg: String| -> SubAgentResult {
            SubAgentResult {
                agent_id: agent_id.clone(),
                output: String::new(),
                tokens: 0,
                turns: 0,
                transcript_path: std::path::PathBuf::new(),
                files_modified: vec![],
                outcome: SubAgentOutcome::Error(msg),
            }
        };

        // Share the parent's LLM client — same OAuth token manager, same connection pool.
        let llm_client = {
            let guard = self.llm_client.read().await;
            guard.clone_boxed()
        };

        // Give all sub-agents the full tool set + shared engram memory.
        // Safety is enforced via system prompt guidance (e.g., "you are a reviewer —
        // focus on review"), not by removing tools. Sub-agents need tools to do their job.
        // Memory sharing lets sub-agents recall context and checkpoint progress.
        let tools = ToolRegistry::for_subagent_with_memory(&workspace_dir, self.memory.clone());
        let workspace = match Workspace::load(&workspace_dir) {
            Ok(w) => w,
            Err(e) => return fail(format!("Failed to load workspace '{}': {}", workspace_dir, e)),
        };

        let subagent = SubAgent {
            id: agent_id.clone(),
            name: format!("{}:{}", agent_type.name, agent_id),
            workspace,
            session_prefix: format!("agent:{}:", agent_id),
            llm_client,
            tools,
            max_iterations,
            system_prompt: Some(agent_type.system_prompt.to_string()),
            model_override: Some(model.to_string()),
        };

        // Inject skill prompt if requested — look up via SkillRegistry and prepend to task
        let effective_task = if let Some(ref skill_name) = options.skill {
            if let Some(skill) = self.workspace.skill_registry.get(skill_name) {
                tracing::info!("Injecting skill '{}' ({} chars) into sub-agent task", skill_name, skill.prompt_content().len());
                // If the skill has a subagent_preamble, inject it between skill instructions and task.
                // This lets skills override generic sub-agent behavior with skill-specific guidance.
                if let Some(ref preamble) = skill.frontmatter.subagent_preamble {
                    tracing::info!("Skill '{}' has subagent_preamble ({} chars)", skill_name, preamble.len());
                    format!(
                        "# Sub-Agent Mode (Skill-Specific)\n\n{}\n\n---\n\n# Skill Instructions\n\n{}\n\n---\n\n# Your Task\n\n{}",
                        preamble, skill.prompt_content(), task
                    )
                } else {
                    format!("# Skill Instructions\n\n{}\n\n---\n\n# Your Task\n\n{}", skill.prompt_content(), task)
                }
            } else {
                tracing::warn!("Skill '{}' not found in SkillRegistry", skill_name);
                task.to_string()
            }
        } else {
            task.to_string()
        };

        // Build user message
        let time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z").to_string();
        let mut user_message = format!(
            "Time: {}\nWorkspace: {}\n\n## Task\n{}",
            time, workspace_dir, effective_task
        );
        for block in &options.context {
            user_message.push_str(&format!("\n\n## {}\n{}", block.label, block.content));
        }

        // Open transcript
        let mut transcript = match TranscriptWriter::open(&agent_id) {
            Ok(tw) => Some(tw),
            Err(e) => {
                tracing::warn!("Failed to open transcript for {}: {}", agent_id, e);
                None
            }
        };

        tracing::info!(
            "run_subagent '{}' type={} model={} workspace={} max_iterations={}",
            agent_id, agent_type.name, model, workspace_dir, max_iterations
        );

        // Execute the agentic loop — classify errors on failure
        let (outcome, output, turns, files_modified) = match self.process_with_subagent(
            &subagent,
            &user_message,
            Some(&agent_id),
            transcript.as_mut(),
        ).await {
            Ok(loop_result) => {
                let outcome = match loop_result.exit_reason {
                    LoopExit::Completed => SubAgentOutcome::Completed,
                    LoopExit::MaxIterations => SubAgentOutcome::MaxIterations,
                    LoopExit::Timeout { elapsed_secs } => SubAgentOutcome::Timeout(format!("{}s", elapsed_secs)),
                    LoopExit::Cancelled => SubAgentOutcome::Cancelled,
                };
                (outcome, loop_result.output, loop_result.turns, loop_result.files_modified)
            }
            Err(e) => {
                tracing::error!("Sub-agent '{}' failed: {}", agent_id, e);
                (classify_error(&e), String::new(), 0, vec![])
            }
        };

        // Extract token count from session
        let session_key = format!("agent:{}:{}", agent_id, agent_id);
        let tokens = self.sessions.get_session(&session_key).await
            .map(|s| s.total_tokens)
            .unwrap_or(0);

        let transcript_path = transcript
            .map(|tw| tw.path.clone())
            .unwrap_or_default();

        SubAgentResult {
            agent_id,
            output,
            tokens,
            turns,
            transcript_path,
            files_modified,
            outcome,
        }
    }

    pub fn spawn_agent_with_options(
        &self,
        agent_config: &AgentConfig,
        max_iterations: u32,
    ) -> anyhow::Result<SubAgent> {
        // Load workspace from agent config or use default
        let workspace_dir = agent_config
            .workspace
            .as_ref()
            .map(|s| s.as_str())
            .unwrap_or(self.config.workspace.as_deref().unwrap_or("."));
        
        let workspace = Workspace::load(workspace_dir)?;

        // Create LLM client with independent config for sub-agent
        let mut llm_config = self.config.llm.clone();
        if let Some(model) = &agent_config.model {
            llm_config.model = model.clone();
        }
        // Sub-agents need a longer HTTP timeout than the main agent because:
        // 1. Non-streaming API calls block until full response is generated
        // 2. Large accumulated context (tool results) makes API processing slower
        // 3. Opus model can take 2-4 minutes per response on complex tasks
        // The default 120s causes "error sending request" failures at ~turn 13-15.
        llm_config.request_timeout_secs = llm_config.request_timeout_secs.max(300);
        let llm_client = llm::create_client(&llm_config)?;

        // Create sub-agent's own tool registry scoped to its workspace + shared engram
        let tools = ToolRegistry::for_subagent_with_memory(workspace_dir, self.memory.clone());

        let session_prefix = format!("agent:{}:", agent_config.id);
        let name = agent_config
            .name
            .clone()
            .unwrap_or_else(|| agent_config.id.clone());

        tracing::info!(
            "Spawned sub-agent '{}' (workspace: {}, model: {}, max_iterations: {})",
            name,
            workspace_dir,
            llm_config.model,
            max_iterations
        );

        Ok(SubAgent {
            id: agent_config.id.clone(),
            name,
            workspace,
            session_prefix,
            llm_client,
            tools,
            max_iterations,
            system_prompt: None,
            model_override: None,
        })
    }

    /// Process a message using a sub-agent with full agentic loop.
    pub async fn process_with_subagent(
        &self,
        subagent: &SubAgent,
        user_message: &str,
        session_suffix: Option<&str>,
        mut transcript: Option<&mut TranscriptWriter>,
    ) -> anyhow::Result<LoopResult> {
        let session_key = format!(
            "{}{}",
            subagent.session_prefix,
            session_suffix.unwrap_or("main")
        );

        tracing::info!(
            "Sub-agent '{}' processing (max_iterations={}): {}",
            subagent.name,
            subagent.max_iterations,
            { let _end = user_message.len().min(50); let _end = user_message.floor_char_boundary(_end); &user_message[.._end] }
        );

        // Get or create session
        let mut session = self.sessions.get_or_create(&session_key).await;

        // Build focused subagent system prompt (no workspace files, task-focused)
        // Use agent type's constant prompt if set (cache-friendly), else generic fallback
        let system_prompt = match &subagent.system_prompt {
            Some(sp) => sp.clone(),
            None => subagent.workspace.build_subagent_system_prompt(user_message),
        };

        // Add user message
        session.messages.push(Message::text("user", user_message));

        // Trim messages
        session.trim_messages(self.config.max_session_messages);

        // Microcompact old tool results
        crate::session::microcompact_messages(&mut session.messages, &self.config.context);

        // Get tool definitions from sub-agent's own registry
        let tool_defs = subagent.tools.definitions();

        // Full agentic loop (same pattern as main agent, with auto-compact)
        let max_turns = subagent.max_iterations as usize;
        let mut response_text = String::new();
        let mut completed_turns: u32 = 0;
        let mut files_modified: Vec<String> = Vec::new();
        let mut loop_exit = LoopExit::Completed;

        // Register cancellation token for sub-agent so it can be cancelled externally
        let cancel_token = self.get_cancellation_token(&session_key).await;

        // Track parent→child relationship for cascade cancel
        {
            let parent_key = self.tools.current_session_key.lock()
                .ok()
                .and_then(|g| g.clone())
                .unwrap_or_else(|| "unknown".to_string());
            let mut children = self.subagent_children.lock().await;
            children.entry(parent_key).or_default().push(session_key.clone());
        }

        // Wall-clock timeout: sub-agents get 20 minutes max.
        // 10 min was too tight — a single LLM call can take 5 min (300s timeout),
        // and retries add more dead time. With 15-turn loops, need enough headroom.
        let wall_clock_start = std::time::Instant::now();
        let wall_clock_limit = std::time::Duration::from_secs(1200); // 20 minutes

        // ISS-012: Track write operations to detect "all-read-no-write" anti-pattern.
        // Sub-agents that spend all iterations reading files and never write output
        // get warned at 50% and 75% to start writing immediately.
        let mut write_call_count: usize = 0;

        for turn in 0..max_turns {
            // Check wall-clock timeout
            if wall_clock_start.elapsed() > wall_clock_limit {
                tracing::warn!(
                    "Sub-agent '{}' wall-clock timeout ({:.0}s > {}s) at turn {}",
                    subagent.name, wall_clock_start.elapsed().as_secs_f64(), wall_clock_limit.as_secs(), turn
                );
                response_text = format!("(Sub-agent timed out after {:.0}s at turn {})", wall_clock_start.elapsed().as_secs_f64(), turn);
                loop_exit = LoopExit::Timeout { elapsed_secs: wall_clock_start.elapsed().as_secs() };
                break;
            }
            // Check if cancelled
            if cancel_token.is_cancelled() {
                tracing::info!("Sub-agent '{}' cancelled at turn {}", subagent.name, turn);
                response_text = format!("(Sub-agent cancelled at turn {})", turn);
                loop_exit = LoopExit::Cancelled;
                break;
            }
            // Iteration awareness: inject a warning when approaching the limit.
            // At 75% of max iterations, tell the sub-agent to checkpoint progress.
            // This is the "mirror" — the sub-agent can see itself running out of time
            // and act accordingly (store partial results to engram, wrap up).
            let iteration_warning_turn = (max_turns as f64 * 0.75) as usize;
            if turn == iteration_warning_turn && turn > 0 {
                tracing::info!(
                    "Sub-agent '{}' turn {}/{}: injecting iteration budget warning",
                    subagent.name, turn, max_turns
                );
                session.messages.push(Message::text(
                    "user",
                    &format!(
                        "⚠️ ITERATION BUDGET WARNING: You are on turn {}/{} ({:.0}% used). \
                         You have {} turns remaining. If you cannot complete the task in the \
                         remaining turns:\n\
                         1. Use `engram_store` to save your progress (what you've done, what remains, key findings)\n\
                         2. Write any partial results to files\n\
                         3. Provide a clear summary of completed vs remaining work\n\
                         Do NOT waste remaining turns on low-value actions. Focus on the most critical remaining work.",
                        turn, max_turns,
                        (turn as f64 / max_turns as f64) * 100.0,
                        max_turns - turn
                    ),
                ));
            }

            // Token-based auto-compact check before each LLM call.
            //
            // IMPORTANT: session.estimate_tokens() only counts message content chars/4.
            // The actual API request includes system prompt + tool schemas + JSON overhead,
            // which can add 5-10K tokens. We add a fixed overhead to the estimate so
            // compact triggers before the request actually fails.
            {
                let model_limit = crate::llm::model_context_limit(
                    subagent.llm_client.model_name()
                );
                let subagent_compact_pct = self.config.context.compact_threshold_pct.min(0.60);
                let threshold = (model_limit as f64 * subagent_compact_pct) as usize;
                // Add estimated overhead: system prompt (~200 tokens) + tool schemas
                // (~150 tokens per tool × ~10 tools) + JSON structure overhead (~500 tokens)
                let tool_overhead = tool_defs.len() * 150;
                let fixed_overhead = 200 + tool_overhead + 500 + system_prompt.len() / 4;
                let estimated = session.estimate_tokens() + fixed_overhead;
                if estimated > threshold {
                    tracing::info!(
                        "Sub-agent '{}' turn {}: auto-compact triggered ({} tokens > {} threshold)",
                        subagent.name, turn, estimated, threshold
                    );
                    match Self::compact_session_with_llm(subagent.llm_client.as_ref(), &mut session, &self.config.context).await {
                        Ok(true) => {
                            tracing::info!("Sub-agent '{}' auto-compact succeeded", subagent.name);
                        }
                        Ok(false) => {}
                        Err(e) => {
                            tracing::warn!("Sub-agent '{}' auto-compact failed: {} — trimming instead", subagent.name, e);
                            session.trim_messages(self.config.max_session_messages);
                        }
                    }
                    crate::session::microcompact_messages(&mut session.messages, &self.config.context);
                }
            }

            // Use streaming + collect to avoid HTTP timeout on large contexts.
            // Non-streaming `chat()` has a fixed timeout for the entire response,
            // which fails when the model takes long to generate (common with opus or
            // large accumulated context). Streaming keeps the connection alive as chunks
            // arrive, so there's no generation-time timeout.
            //
            // Streaming refusal risk: Claude 4 triggers refusal on specific system prompt
            // strings ("You are OpenCode", "You are a personal assistant running inside
            // OpenClaw."). Sub-agent prompt ("You are a **subagent** spawned by the main
            // agent") does NOT match any trigger — safe to stream.
            // Use model override if set (sub-agents sharing parent's client use different models)
            let effective_model = subagent.model_override.as_deref()
                .unwrap_or(subagent.llm_client.model_name());

            let response = match subagent
                .llm_client
                .chat_stream_with_model(&system_prompt, &session.messages, &tool_defs, effective_model)
                .await
            {
                Ok(rx) => {
                    let resp = crate::llm::collect_stream(rx).await?;
                    if resp.stop_reason == "refusal" {
                        tracing::error!(
                            "Sub-agent '{}' turn {}: streaming refusal detected! Falling back to non-streaming.",
                            subagent.name, turn
                        );
                        subagent.llm_client.chat_with_model(&system_prompt, &session.messages, &tool_defs, effective_model).await?
                    } else {
                        resp
                    }
                }
                Err(e) if self.config.context.reactive_compact && crate::llm::is_prompt_too_long(&e) => {
                    tracing::warn!("Sub-agent '{}' turn {}: stream send error — reactive compact", subagent.name, turn);
                    match Self::compact_session_with_llm(subagent.llm_client.as_ref(), &mut session, &self.config.context).await {
                        Ok(true) => {
                            let rx = subagent.llm_client.chat_stream_with_model(&system_prompt, &session.messages, &tool_defs, effective_model).await?;
                            crate::llm::collect_stream(rx).await?
                        }
                        _ => return Err(e),
                    }
                }
                Err(e) => return Err(e),
            };

            session.total_tokens +=
                (response.usage.input_tokens + response.usage.output_tokens) as u64;

            tracing::info!(
                "Sub-agent '{}' turn {}: in={} out={} cache_read={} cache_write={} stop={:?} tool_calls={} text_len={}",
                subagent.name,
                turn,
                response.usage.input_tokens,
                response.usage.output_tokens,
                response.usage.cache_read,
                response.usage.cache_write,
                response.stop_reason,
                response.tool_calls.len(),
                response.text.as_ref().map(|t| t.len()).unwrap_or(0)
            );

            if let Some(text) = &response.text {
                response_text = text.clone();
            }

            // If max_tokens hit during tool call, the JSON is likely truncated.
            // Don't try to execute — ask LLM to retry with smaller steps.
            if response.stop_reason == "max_tokens" && !response.tool_calls.is_empty() {
                tracing::warn!(
                    "Sub-agent '{}' turn {}: max_tokens hit during tool call — output truncated. Asking to retry.",
                    subagent.name, turn
                );
                session.messages.push(Message::text(
                    "user",
                    "Your last response was truncated (hit output token limit). \
                     Break the work into smaller steps — write shorter content per tool call, \
                     or split large files into multiple write_file calls.",
                ));
                continue;
            }

            if response.tool_calls.is_empty() {
                // No tool calls — add final assistant message and break
                if !response_text.is_empty() {
                    tracing::info!(
                        "Sub-agent '{}' final response ({} chars): {}...",
                        subagent.name,
                        response_text.len(),
                        {
                            let end = response_text.len().min(100);
                            let end = response_text.floor_char_boundary(end);
                            &response_text[..end]
                        }
                    );
                    session
                        .messages
                        .push(Message::text("assistant", &response_text));
                }
                break;
            }

            // Add assistant message with tool calls
            tracing::info!(
                "Sub-agent '{}' turn {}: {} tool call(s)",
                subagent.name,
                turn,
                response.tool_calls.len()
            );
            session.messages.push(Message::assistant_with_tools(
                response.text.as_deref(),
                response.tool_calls.clone(),
            ));

            // Execute each tool using sub-agent's tool registry
            let mut tool_results = Vec::new();
            let mut tool_names_for_persist: Vec<String> = Vec::new();
            for tc in &response.tool_calls {
                tool_names_for_persist.push(tc.name.clone());
                let result = subagent.tools.execute(&tc.name, tc.input.clone()).await;
                match result {
                    Ok(tool_result) => {
                        tracing::info!(
                            "Sub-agent '{}' tool {} → {} chars, error={}",
                            subagent.name,
                            tc.name,
                            tool_result.output.len(),
                            tool_result.is_error
                        );
                        // Track successfully modified files
                        if !tool_result.is_error && (tc.name == "write_file" || tc.name == "edit_file") {
                            if let Some(path) = tc.input.get("path").and_then(|v| v.as_str()) {
                                if !files_modified.contains(&path.to_string()) {
                                    files_modified.push(path.to_string());
                                }
                            }
                            write_call_count += 1;
                        }
                        tool_results.push((tc.id.clone(), tool_result.output, tool_result.is_error));
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Sub-agent '{}' tool {} error: {}",
                            subagent.name,
                            tc.name,
                            e
                        );
                        tool_results.push((tc.id.clone(), format!("Tool error: {}", e), true));
                    }
                }
            }

            // Persist large tool results to disk (sub-agents use lower threshold: 10K vs 30K)
            {
                let mut subagent_context_config = self.config.context.clone();
                subagent_context_config.persist_threshold = subagent_context_config.persist_threshold.min(10_000);
                persist_large_tool_results(&session_key, &mut tool_results, &tool_names_for_persist, &subagent_context_config);
            }

            // Add tool results as user message
            session.messages.push(Message::tool_results(tool_results));

            // Transcript: log assistant text + tool call names (not full results)
            if let Some(tw) = transcript.as_mut() {
                let tool_names: Vec<&str> = response.tool_calls.iter().map(|tc| tc.name.as_str()).collect();
                let summary = format!("tools: [{}]", tool_names.join(", "));
                if let Some(ref text) = response.text {
                    let _ = tw.append("assistant", &format!("{}\n{}", text, summary));
                } else {
                    let _ = tw.append("assistant", &summary);
                }
            }

            completed_turns += 1;

            // ISS-012: Write-tracking budget warning.
            // If we're past 50% of iterations and haven't written anything, warn the sub-agent.
            // This catches the "all-read-no-write" anti-pattern before it's too late.
            if write_call_count == 0 && max_turns > 4 {
                let progress = (turn + 1) as f64 / max_turns as f64;
                let remaining = max_turns - turn - 1;
                if progress >= 0.75 {
                    tracing::warn!(
                        "Sub-agent '{}' turn {}/{}: CRITICAL — 75% iterations used, zero write operations",
                        subagent.name, turn + 1, max_turns
                    );
                    session.messages.push(Message::text(
                        "user",
                        &format!(
                            "🚨 CRITICAL: You have used {}/{} iterations ({:.0}%) without writing ANY output files. \
                             You MUST start writing your output NOW. You have only {} iterations remaining. \
                             All pre-loaded files in your context are your input — stop reading and START WRITING. \
                             Partial output is better than no output.",
                            turn + 1, max_turns, progress * 100.0, remaining
                        ),
                    ));
                } else if progress >= 0.5 {
                    tracing::warn!(
                        "Sub-agent '{}' turn {}/{}: WARNING — 50% iterations used, zero write operations",
                        subagent.name, turn + 1, max_turns
                    );
                    session.messages.push(Message::text(
                        "user",
                        &format!(
                            "⚠️ BUDGET WARNING: You have used {}/{} iterations ({:.0}%) without writing any output files. \
                             Pre-loaded files in your context ARE your input — do not re-read them. \
                             START WRITING your output file now. Budget: {} iterations remaining.",
                            turn + 1, max_turns, progress * 100.0, remaining
                        ),
                    ));
                }
            }
        }

        // If we hit max iterations without a final response, note it
        if response_text.is_empty() && matches!(loop_exit, LoopExit::Completed) {
            tracing::warn!(
                "Sub-agent '{}' reached max iterations ({}) without final response",
                subagent.name,
                max_turns
            );
            response_text = format!(
                "[Sub-agent '{}' reached maximum iterations ({}) without completing]",
                subagent.name,
                max_turns
            );
            loop_exit = LoopExit::MaxIterations;

            // Auto-checkpoint: store partial progress to engram so the main agent
            // can resume or delegate to a new sub-agent with context.
            let checkpoint_summary = format!(
                "Sub-agent '{}' hit max iterations ({}) on task: {}... | Files modified: {:?} | Completed turns: {}",
                subagent.name,
                max_turns,
                { let end = user_message.len().min(200); let end = user_message.floor_char_boundary(end); &user_message[..end] },
                files_modified,
                completed_turns
            );
            if let Err(e) = self.memory.store(
                &checkpoint_summary,
                engramai::MemoryType::Episodic,
                0.7,
                Some("sub-agent-checkpoint"),
            ) {
                tracing::warn!("Failed to store sub-agent checkpoint to engram: {}", e);
            } else {
                tracing::info!("Stored sub-agent checkpoint to engram for '{}'", subagent.name);
            }
        }

        // Update session
        self.sessions.update(session).await;

        // Clean up cancellation token
        {
            let mut tokens = self.cancellation_tokens.lock().await;
            tokens.remove(&session_key);
        }

        // Clean up parent→child tracking
        {
            let mut children = self.subagent_children.lock().await;
            for child_list in children.values_mut() {
                child_list.retain(|k| k != &session_key);
            }
            // Remove empty parent entries
            children.retain(|_, v| !v.is_empty());
        }

        Ok(LoopResult {
            output: response_text,
            turns: completed_turns,
            files_modified,
            exit_reason: loop_exit,
        })
    }

    /// Get the current ritual ToolScope, if any ritual is active.
    ///
    /// Returns None if no ritual is active (= no constraints).
    fn get_active_scope(&self) -> Option<gid_core::ritual::ToolScope> {
        use gid_core::ritual::{default_scope_for_phase, rustclaw_tool_mapping};

        let ritual_state_path = self.workspace.root.join(".gid/ritual-state.json");
        let ritual_def_path = self.workspace.root.join(".gid/ritual.yml");

        if !ritual_state_path.exists() || !ritual_def_path.exists() {
            return None;
        }

        let state: gid_core::ritual::RitualState = std::fs::read_to_string(&ritual_state_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())?;

        let definition: gid_core::ritual::RitualDefinition = std::fs::read_to_string(&ritual_def_path)
            .ok()
            .and_then(|s| serde_yaml::from_str(&s).ok())?;

        match &state.status {
            gid_core::ritual::RitualStatus::Running
            | gid_core::ritual::RitualStatus::WaitingApproval { .. } => {},
            _ => return None,
        }

        let phase_id = definition.phases
            .get(state.current_phase)
            .map(|p| p.id.as_str())
            .unwrap_or("unknown");

        let mapping = rustclaw_tool_mapping();
        Some(default_scope_for_phase(phase_id).with_tool_mapping(&mapping))
    }

    /// Validate a tool call against the active ritual scope.
    ///
    /// Returns Ok(()) if allowed, Err(reason) if blocked.
    /// This is the second enforcement layer: path constraints and bash policy.
    /// (First layer is tool visibility filtering in apply_ritual_scope.)
    fn check_tool_call_scope(
        &self,
        scope: &gid_core::ritual::ToolScope,
        tool_name: &str,
        input: &serde_json::Value,
    ) -> Result<(), String> {
        // Extract path for write/edit operations
        let path = input.get("path")
            .or_else(|| input.get("file_path"))
            .and_then(|v| v.as_str());

        match tool_name {
            "write_file" | "edit_file" => {
                if let Some(path) = path {
                    // Normalize: strip workspace root if present
                    let rel_path = path.strip_prefix(
                        self.workspace.root.to_str().unwrap_or("")
                    ).unwrap_or(path).trim_start_matches('/');

                    if !scope.is_path_writable(rel_path) {
                        return Err(format!(
                            "Ritual scope violation: cannot write to '{}' in current phase. \
                             Allowed paths: {:?}",
                            rel_path, scope.writable_paths
                        ));
                    }
                }
            }
            "read_file" => {
                if let Some(path) = path {
                    let rel_path = path.strip_prefix(
                        self.workspace.root.to_str().unwrap_or("")
                    ).unwrap_or(path).trim_start_matches('/');

                    if !scope.is_path_readable(rel_path) {
                        return Err(format!(
                            "Ritual scope violation: cannot read '{}' in current phase.",
                            rel_path
                        ));
                    }
                }
            }
            "exec" => {
                let command = input.get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if !scope.is_bash_allowed(command) {
                    return Err(format!(
                        "Ritual scope violation: command '{}' not allowed in current phase. \
                         Allowed: {:?}",
                        command, scope.bash_policy
                    ));
                }
            }
            _ => {} // Other tools: no additional constraints
        }

        Ok(())
    }

    /// Apply ritual ToolScope to filter tool definitions based on current phase.
    ///
    /// Layer 1: Tool visibility — LLM doesn't see tools not in scope.
    fn apply_ritual_scope(&self, tools: Vec<crate::llm::ToolDefinition>) -> Vec<crate::llm::ToolDefinition> {
        let scope = match self.get_active_scope() {
            Some(s) => s,
            None => return tools,
        };

        let original_count = tools.len();
        let filtered = scope.filter_tools(tools, |t| t.name.as_str());
        let filtered_count = filtered.len();

        if filtered_count < original_count {
            tracing::info!(
                total = original_count,
                allowed = filtered_count,
                removed = original_count - filtered_count,
                "ToolScope Layer 1 — filtered tool visibility"
            );
        }

        filtered
    }

    /// Get all configured agents.
    /// Get session manager reference.
    pub fn sessions(&self) -> &SessionManager {
        &self.sessions
    }

    /// Get the workspace root path.
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub fn workspace_root(&self) -> &std::path::Path {
        &self.workspace.root
    }

    /// Get a shared reference to the LLM client.
    pub fn llm_client(&self) -> Arc<RwLock<Box<dyn LlmClient>>> {
        self.llm_client.clone()
    }

    /// Get embedding service status (for dashboard).
    pub fn embedding_status(&self) -> String {
        self.memory.embedding_status()
    }

    pub fn agents(&self) -> &[AgentConfig] {
        &self.config.agents
    }

    /// Find an agent config by ID.
    pub fn find_agent(&self, id: &str) -> Option<&AgentConfig> {
        self.config.agents.iter().find(|a| a.id == id)
    }

    /// Get the default agent config (if any).
    pub fn default_agent(&self) -> Option<&AgentConfig> {
        self.config.agents.iter().find(|a| a.default)
    }

    /// Process a message with streaming response.
    /// Returns a channel that emits partial text chunks, then final complete response.
    /// Only the final response (after all tool calls) is streamed.
    pub async fn process_message_streaming(
        &self,
        session_key: &str,
        user_message: &str,
        user_id: Option<&str>,
        channel: Option<&str>,
    ) -> anyhow::Result<mpsc::Receiver<String>> {
        let (tx, rx) = mpsc::channel::<String>(100);
        
        // Clone what we need for the spawned task
        let session_key = session_key.to_string();
        let user_message = user_message.to_string();
        let user_id = user_id.map(String::from);
        let channel = channel.map(String::from);
        
        // We need to process in this function because we can't easily move self into async
        // So we'll do the tool loop here and only stream the final response
        
        tracing::info!(
            "Processing streaming message for session={} user={:?}",
            session_key,
            user_id
        );

        // 1. Get or create session
        let mut session = self.sessions.get_or_create(&session_key).await;

        // 2. Run BeforeInbound hooks
        let mut hook_ctx = HookContext {
            session_key: session_key.clone(),
            user_id: user_id.clone(),
            channel: channel.clone(),
            content: user_message.clone(),
            metadata: serde_json::json!({}),
        };

        {
            let hooks = self.hooks.read().await;
            if let HookOutcome::Reject(reason) =
                hooks.run(HookPoint::BeforeInbound, &mut hook_ctx).await?
            {
                tx.send(format!("Message rejected: {}", reason)).await.ok();
                return Ok(rx);
            }
        }

        // 3. Recall relevant memories
        let memory_context = {
            match self.memory.recall(&user_message) {
                Ok(memories) if !memories.is_empty() => {
                    tracing::info!("Recalled {} memories", memories.len());
                    MemoryManager::format_for_prompt(&memories)
                }
                _ => String::new(),
            }
        };

        // 4. Build system prompt (pass user message for dynamic skill injection)
        let mut system_prompt = self.workspace.build_system_prompt_with_skills(false, Some(&user_message));
        if !memory_context.is_empty() {
            system_prompt.push_str("\n\n## Relevant Memories\n");
            system_prompt.push_str(&memory_context);
        }

        // 5. Add user message to session
        session.messages.push(Message::text("user", &user_message));

        // 6. Trim messages
        session.trim_messages(self.config.max_session_messages);

        // 7. Microcompact old tool results
        crate::session::microcompact_messages(&mut session.messages, &self.config.context);

        // 8. Get tool definitions (filtered by ritual phase if active)
        let tool_defs = self.apply_ritual_scope(self.tools.definitions());

        // 9. Agentic loop - non-streaming until final response
        let max_turns = 80;
        let mut has_tool_calls = true;

        for turn in 0..max_turns {
            if !has_tool_calls {
                break;
            }

            let response = self
                .llm_client
                .read().await
                .chat(&system_prompt, &session.messages, &tool_defs)
                .await?;

            session.total_tokens +=
                (response.usage.input_tokens + response.usage.output_tokens) as u64;

            if response.tool_calls.is_empty() {
                has_tool_calls = false;
                
                // This is the final response - now we stream it
                // But we already have the full response, so we'd need to re-request with streaming
                // For simplicity, just send the complete response
                if let Some(text) = &response.text {
                    session.messages.push(Message::text("assistant", text));
                    tx.send(text.clone()).await.ok();
                }
                break;
            }

            // Add assistant message with tool calls
            tracing::info!("Turn {}: {} tool call(s)", turn, response.tool_calls.len());
            session.messages.push(Message::assistant_with_tools(
                response.text.as_deref(),
                response.tool_calls.clone(),
            ));

            // Execute each tool
            let mut tool_results = Vec::new();
            let mut tool_names_for_persist: Vec<String> = Vec::new();
            for tc in &response.tool_calls {
                tool_names_for_persist.push(tc.name.clone());
                // Run BeforeToolCall hook
                let mut tc_ctx = HookContext {
                    session_key: session_key.clone(),
                    user_id: user_id.clone(),
                    channel: channel.clone(),
                    content: tc.name.clone(),
                    metadata: tc.input.clone(),
                };

                {
                    let hooks = self.hooks.read().await;
                    if let HookOutcome::Reject(reason) =
                        hooks.run(HookPoint::BeforeToolCall, &mut tc_ctx).await?
                    {
                        tool_results.push((
                            tc.id.clone(),
                            format!("Tool call rejected: {}", reason),
                            true,
                        ));
                        continue;
                    }
                }

                // Execute tool with sandbox enforcement
                let result = self.execute_tool_sandboxed(&tc.name, tc.input.clone()).await;
                match result {
                    Ok(tool_result) => {
                        let sanitized = self.safety.sanitize_tool_output(&tc.name, &tool_result.output);
                        tracing::info!(
                            "Tool {} → {} chars{}, error={}",
                            tc.name, sanitized.content.len(),
                            if sanitized.was_modified { " (sanitized)" } else { "" },
                            tool_result.is_error
                        );
                        let output = if tc.name == "web_fetch" {
                            wrap_external_content("web_fetch", &sanitized.content)
                        } else {
                            sanitized.content
                        };
                        tool_results.push((tc.id.clone(), output, tool_result.is_error));
                    }
                    Err(e) => {
                        tracing::warn!("Tool {} sandbox error: {}", tc.name, e);
                        tool_results.push((tc.id.clone(), format!("Sandbox error: {}", e), true));
                    }
                }
            }

            // Persist large tool results to disk
            persist_large_tool_results(&session_key, &mut tool_results, &tool_names_for_persist, &self.config.context);

            // Add tool results as user message
            session.messages.push(Message::tool_results(tool_results));
        }

        // Now stream the final response
        if has_tool_calls {
            // We finished the tool loop, now get final streaming response
            let mut stream_rx = self
                .llm_client
                .read().await
                .chat_stream(&system_prompt, &session.messages, &[])
                .await?;

            let mut final_text = String::new();
            while let Some(chunk) = stream_rx.recv().await {
                match chunk {
                    StreamChunk::Text(text) => {
                        final_text.push_str(&text);
                        tx.send(text).await.ok();
                    }
                    StreamChunk::Done(usage, _) => {
                        session.total_tokens +=
                            (usage.input_tokens + usage.output_tokens) as u64;
                        break;
                    }
                    StreamChunk::ToolUse(_) => {
                        // Shouldn't happen in final response
                    }
                }
            }

            if !final_text.is_empty() {
                session.messages.push(Message::text("assistant", &final_text));
            }
        }

        // 9. Run BeforeOutbound hooks
        {
            let out_ctx = HookContext {
                session_key: session_key.clone(),
                user_id: user_id.clone(),
                channel: channel.clone(),
                content: String::new(), // We've already sent chunks
                metadata: serde_json::json!({
                    "user_message": user_message,
                }),
            };
            let hooks = self.hooks.read().await;
            let _ = hooks.run(HookPoint::BeforeOutbound, &mut out_ctx.clone()).await;
        }

        // 10. Update session
        self.sessions.update(session).await;

        Ok(rx)
    }
}
