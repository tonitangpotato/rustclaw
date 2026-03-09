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

use crate::config::Config;
use crate::hooks::{HookContext, HookOutcome, HookPoint, HookRegistry};
use crate::llm::{self, LlmClient, Message};
use crate::memory::MemoryManager;
use crate::session::SessionManager;
use crate::tools::ToolRegistry;
use crate::workspace::Workspace;

/// The core agent runner.
pub struct AgentRunner {
    config: Config,
    workspace: Workspace,
    memory: Arc<RwLock<MemoryManager>>,
    sessions: SessionManager,
    hooks: Arc<RwLock<HookRegistry>>,
    tools: ToolRegistry,
    llm_client: Box<dyn LlmClient>,
}

impl AgentRunner {
    pub fn new(
        config: Config,
        workspace: Workspace,
        memory: MemoryManager,
        sessions: SessionManager,
        hooks: HookRegistry,
        tools: ToolRegistry,
    ) -> Self {
        let llm_client = llm::create_client(&config.llm).expect("Failed to create LLM client");

        Self {
            config,
            workspace,
            memory: Arc::new(RwLock::new(memory)),
            sessions,
            hooks: Arc::new(RwLock::new(hooks)),
            tools,
            llm_client,
        }
    }

    /// Process an incoming message and return a response.
    pub async fn process_message(
        &self,
        session_key: &str,
        user_message: &str,
        user_id: Option<&str>,
        channel: Option<&str>,
    ) -> anyhow::Result<String> {
        tracing::info!(
            "Processing message for session={} user={:?}",
            session_key,
            user_id
        );

        // 1. Get or create session
        let mut session = self.sessions.get_or_create(session_key).await;

        // 2. Run BeforeInbound hooks
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

        // 3. Recall relevant memories
        let memory_context = {
            let mut mem = self.memory.write().await;
            match mem.recall(user_message) {
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
            system_prompt.push_str("\n\n");
            system_prompt.push_str(&memory_context);
        }

        // 5. Add user message to session
        session.messages.push(Message::text("user", user_message));

        // 6. Get tool definitions
        let tool_defs = self.tools.definitions();

        // 7. Agentic loop
        let max_turns = 30;
        let mut response_text = String::new();

        for turn in 0..max_turns {
            let response = self
                .llm_client
                .chat(&system_prompt, &session.messages, &tool_defs)
                .await?;

            session.total_tokens +=
                (response.usage.input_tokens + response.usage.output_tokens) as u64;

            if let Some(text) = &response.text {
                response_text = text.clone();
            }

            if response.tool_calls.is_empty() {
                // No tool calls — add final assistant message and break
                if !response_text.is_empty() {
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

                let result = self.tools.execute(&tc.name, tc.input.clone()).await?;
                tracing::info!(
                    "Tool {} → {} chars, error={}",
                    tc.name,
                    result.output.len(),
                    result.is_error
                );

                tool_results.push((tc.id.clone(), result.output, result.is_error));
            }

            // Add tool results as user message
            session.messages.push(Message::tool_results(tool_results));
        }

        // 8. Run BeforeOutbound hooks
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

        // 9. Update session
        self.sessions.update(session).await;

        Ok(response_text)
    }
}
