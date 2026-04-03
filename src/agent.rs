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
}

/// The core agent runner.
pub struct AgentRunner {
    config: Config,
    workspace: Workspace,
    memory: Arc<MemoryManager>,
    sessions: SessionManager,
    hooks: Arc<RwLock<HookRegistry>>,
    tools: ToolRegistry,
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
    /// Tool call frequency and duration statistics
    pub tool_stats: Arc<crate::tool_stats::ToolStatsTracker>,
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
            tool_stats: Arc::new(crate::tool_stats::ToolStatsTracker::new()),
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
        user_id: Option<&str>,
        channel: Option<&str>,
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
            system_prompt.push_str("\n\n## Relevant Memories\n");
            system_prompt.push_str(&memory_context);
        }

        // 5. Add user message to session
        session.messages.push(Message::text("user", user_message));

        // 6. Summarize or trim messages
        if let Some(ref summary_llm) = self.summary_llm {
            match summarize_old_messages(
                &mut session,
                self.config.max_session_messages,
                summary_llm.as_ref(),
            )
            .await
            {
                Ok(true) => {
                    tracing::info!("Summarized old messages in session {}", session_key);
                }
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!("Summarization failed, falling back to trim: {}", e);
                    session.trim_messages(self.config.max_session_messages);
                }
            }
        } else {
            session.trim_messages(self.config.max_session_messages);
        }

        // 7. Microcompact old tool results
        crate::session::microcompact_messages(&mut session.messages, &self.config.context);

        // 8. Get tool definitions (filtered by ritual phase if active)
        let tool_defs = self.apply_ritual_scope(self.tools.definitions());

        // 9. Agentic loop
        let max_turns = 80;
        let mut response_text = String::new();
        let mut sent_response = false;
        let cancel_token = self.get_cancellation_token(session_key).await;

        for turn in 0..max_turns {
            // Check for cancellation before each turn
            if cancel_token.is_cancelled() {
                tracing::info!("Session {} cancelled at turn {}", session_key, turn);
                let _ = tx.send(AgentEvent::Response("⛔ Stopped.".to_string())).await;
                sent_response = true;
                break;
            }
            // Race LLM call against cancellation token
            let llm_guard = self.llm_client.read().await;
            let llm_future = llm_guard.chat(&system_prompt, &session.messages, &tool_defs);
            let response = tokio::select! {
                res = llm_future => {
                    drop(llm_guard);
                    res?
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

            // Handle max_tokens truncation during tool calls
            if response.stop_reason == "max_tokens" && !response.tool_calls.is_empty() {
                tracing::warn!(
                    "Turn {}: max_tokens hit during tool call — output truncated. Asking to retry.",
                    turn
                );
                session.messages.push(Message::text(
                    "user",
                    "Your last response was truncated (hit output token limit). \
                     Break the work into smaller steps — write shorter content per tool call.",
                ));
                continue;
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

        

        // Safety net: if loop exhausted max_turns without sending Response, send now
        if !sent_response {
            tracing::warn!("Max turns ({}) exhausted without end_turn — sending accumulated response", max_turns);
            if !response_text.is_empty() {
                session.messages.push(Message::text("assistant", &response_text));
            }
            let _ = tx.send(AgentEvent::Response(response_text.clone())).await;
        }

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
                }),
            };
            let hooks_guard = self.hooks.read().await;
            hooks_guard.run(HookPoint::BeforeOutbound, &mut out_ctx).await?;
        }

        // 11. Update session
        self.sessions.update(session).await;

        Ok(())
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
        // Sub-agent inherits parent's LLM config as-is (including max_tokens).
        // With None (default), provider auto-resolves to model max.
        let llm_client = llm::create_client(&llm_config)?;

        // Create sub-agent's own tool registry scoped to its workspace
        let tools = ToolRegistry::for_subagent(workspace_dir);

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
        })
    }

    /// Process a message using a sub-agent with full agentic loop.
    pub async fn process_with_subagent(
        &self,
        subagent: &SubAgent,
        user_message: &str,
        session_suffix: Option<&str>,
    ) -> anyhow::Result<String> {
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
        let system_prompt = subagent.workspace.build_subagent_system_prompt(user_message);

        // Add user message
        session.messages.push(Message::text("user", user_message));

        // Trim messages
        session.trim_messages(self.config.max_session_messages);

        // Microcompact old tool results
        crate::session::microcompact_messages(&mut session.messages, &self.config.context);

        // Get tool definitions from sub-agent's own registry
        let tool_defs = subagent.tools.definitions();

        // Full agentic loop (same pattern as main agent)
        let max_turns = subagent.max_iterations as usize;
        let mut response_text = String::new();

        for turn in 0..max_turns {
            let response = subagent
                .llm_client
                .chat(&system_prompt, &session.messages, &tool_defs)
                .await?;

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

            // Persist large tool results to disk
            persist_large_tool_results(&session_key, &mut tool_results, &tool_names_for_persist, &self.config.context);

            // Add tool results as user message
            session.messages.push(Message::tool_results(tool_results));
        }

        // If we hit max iterations without a final response, note it
        if response_text.is_empty() {
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
        }

        // Update session
        self.sessions.update(session).await;

        Ok(response_text)
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
