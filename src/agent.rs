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
            summary_config.max_tokens = 1024; // Summaries don't need many tokens
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
        }
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

    /// Process an incoming message and return a response.
    pub async fn process_message(
        &self,
        session_key: &str,
        user_message: &str,
        user_id: Option<&str>,
        channel: Option<&str>,
    ) -> anyhow::Result<String> {
        self.process_message_with_options(session_key, user_message, user_id, channel, false).await
    }

    /// Process an incoming message with additional options.
    pub async fn process_message_with_options(
        &self,
        session_key: &str,
        user_message: &str,
        user_id: Option<&str>,
        channel: Option<&str>,
        is_heartbeat: bool,
    ) -> anyhow::Result<String> {
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
            return Ok(warning);
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
            let hooks = self.hooks.read().await;
            if let HookOutcome::Reject(reason) =
                hooks.run(HookPoint::BeforeInbound, &mut hook_ctx).await?
            {
                return Ok(format!("Message rejected: {}", reason));
            }
        }

        // 3. Extract recalled memories from hook metadata (set by EngramRecallHook)
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

        // 4. Build system prompt (include HEARTBEAT.md if this is a heartbeat poll)
        let mut system_prompt = self.workspace.build_system_prompt_with_options(is_heartbeat);
        if !memory_context.is_empty() {
            system_prompt.push_str("\n\n## Relevant Memories\n");
            system_prompt.push_str(&memory_context);
        }

        // 5. Add user message to session
        session.messages.push(Message::text("user", user_message));

        // 6. Summarize or trim messages to stay within context window
        if let Some(ref summary_llm) = self.summary_llm {
            // Try to summarize old messages
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
                Ok(false) => {
                    // No summarization needed
                }
                Err(e) => {
                    tracing::warn!("Summarization failed, falling back to trim: {}", e);
                    session.trim_messages(self.config.max_session_messages);
                }
            }
        } else {
            // No summary model configured, just trim
            session.trim_messages(self.config.max_session_messages);
        }

        // 7. Get tool definitions
        let tool_defs = self.tools.definitions();

        // 8. Agentic loop
        let max_turns = 30;
        let mut response_text = String::new();

        for turn in 0..max_turns {
            let response = self
                .llm_client
                .read().await
                .chat(&system_prompt, &session.messages, &tool_defs)
                .await?;

            session.total_tokens +=
                (response.usage.input_tokens + response.usage.output_tokens) as u64;

            tracing::info!(
                "LLM response: tokens={}/{} stop={:?} tool_calls={} text_len={}",
                response.usage.input_tokens,
                response.usage.output_tokens,
                response.stop_reason,
                response.tool_calls.len(),
                response.text.as_ref().map(|t| t.len()).unwrap_or(0)
            );

            if let Some(text) = &response.text {
                response_text = text.clone();
            }

            if response.tool_calls.is_empty() {
                // No tool calls — add final assistant message and break
                if !response_text.is_empty() {
                    tracing::info!("Final response ({} chars): {}...", response_text.len(),
                        {
                            let end = response_text.len().min(100);
                            let end = response_text.floor_char_boundary(end);
                            &response_text[..end]
                        });
                    session
                        .messages
                        .push(Message::text("assistant", &response_text));
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
            for tc in &response.tool_calls {
                // Run BeforeToolCall hook
                let mut tc_ctx = HookContext {
                    session_key: session_key.to_string(),
                    user_id: user_id.map(String::from),
                    channel: channel.map(String::from),
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
                        // Sanitize tool output through SafetyLayer
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
                        // Wrap web_fetch output as untrusted external content
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

            // Add tool results as user message
            session.messages.push(Message::tool_results(tool_results));
        }

        // 9. Run BeforeOutbound hooks (includes EngramStoreHook for auto-store)
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
            let hooks = self.hooks.read().await;
            hooks.run(HookPoint::BeforeOutbound, &mut out_ctx).await?;
        }

        // 10. Update session
        self.sessions.update(session).await;

        Ok(response_text)
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

        // Create LLM client with model override if specified
        let mut llm_config = self.config.llm.clone();
        if let Some(model) = &agent_config.model {
            llm_config.model = model.clone();
        }
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

        // Build system prompt from sub-agent's workspace
        let system_prompt = subagent.workspace.build_system_prompt();

        // Add user message
        session.messages.push(Message::text("user", user_message));

        // Trim messages
        session.trim_messages(self.config.max_session_messages);

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
                "Sub-agent '{}' turn {}: tokens={}/{} stop={:?} tool_calls={} text_len={}",
                subagent.name,
                turn,
                response.usage.input_tokens,
                response.usage.output_tokens,
                response.stop_reason,
                response.tool_calls.len(),
                response.text.as_ref().map(|t| t.len()).unwrap_or(0)
            );

            if let Some(text) = &response.text {
                response_text = text.clone();
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
            for tc in &response.tool_calls {
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

    /// Get all configured agents.
    /// Get session manager reference.
    pub fn sessions(&self) -> &SessionManager {
        &self.sessions
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

        // 4. Build system prompt
        let mut system_prompt = self.workspace.build_system_prompt();
        if !memory_context.is_empty() {
            system_prompt.push_str("\n\n## Relevant Memories\n");
            system_prompt.push_str(&memory_context);
        }

        // 5. Add user message to session
        session.messages.push(Message::text("user", &user_message));

        // 6. Trim messages
        session.trim_messages(self.config.max_session_messages);

        // 7. Get tool definitions
        let tool_defs = self.tools.definitions();

        // 8. Agentic loop - non-streaming until final response
        let max_turns = 30;
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
            for tc in &response.tool_calls {
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
