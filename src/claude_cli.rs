//! Claude CLI proxy backend.
//!
//! Routes LLM calls through `claude -p` (Claude Code CLI in headless mode)
//! instead of direct API calls. This uses the Max subscription instead of
//! incurring extra usage charges for third-party API access.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::Deserialize;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tokio::sync::{mpsc, Mutex};

use crate::config::ClaudeCliConfig;
use crate::llm::{
    token_tracker, ContentBlock, LlmClient, LlmResponse, Message, StreamChunk, ToolCall,
    ToolDefinition, Usage,
};

/// CC session tracking for `--resume` multi-turn support.
#[derive(Debug, Clone)]
struct CcSession {
    session_id: String,
    #[allow(dead_code)]
    created_at: std::time::Instant,
    last_used: std::time::Instant,
    #[allow(dead_code)]
    turn_count: u32,
}

/// LLM client that proxies through `claude -p` CLI.
pub struct ClaudeCliClient {
    model: String,
    claude_bin: String,
    timeout_secs: u64,
    max_turns: u32,
    session_ttl_secs: u64,
    /// Map of conversation_key -> CC session for --resume.
    sessions: Arc<Mutex<HashMap<String, CcSession>>>,
}

// -- CC JSON output structures --

/// Top-level JSON output from `claude -p --output-format json`.
#[derive(Debug, Deserialize)]
struct CcJsonResult {
    #[serde(default)]
    result: String,
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    usage: CcUsage,
}

/// Usage info from CC output.
#[derive(Debug, Deserialize, Default)]
struct CcUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
    #[serde(default, alias = "cache_read_input_tokens")]
    cache_read: u32,
    #[serde(default, alias = "cache_creation_input_tokens")]
    cache_write: u32,
}

// -- CC stream-json event structures --

/// A line from `claude -p --output-format stream-json --verbose`.
#[derive(Debug, Deserialize)]
struct CcStreamEvent {
    #[serde(rename = "type")]
    type_: String,
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    message: Option<CcStreamMessage>,
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    usage: Option<CcUsage>,
}

#[derive(Debug, Deserialize)]
struct CcStreamMessage {
    #[serde(default)]
    content: Vec<CcContentBlock>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum CcContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(other)]
    Other,
}

impl ClaudeCliClient {
    pub fn new(model: String, cli_config: &ClaudeCliConfig) -> Self {
        Self {
            model,
            claude_bin: cli_config.binary.clone(),
            timeout_secs: cli_config.timeout_secs,
            max_turns: cli_config.max_turns,
            session_ttl_secs: cli_config.session_ttl_hours * 3600,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Serialize messages into a single prompt string for `claude -p`.
    fn serialize_messages(messages: &[Message]) -> String {
        let mut parts = Vec::new();
        for msg in messages {
            let role = match msg.role.as_str() {
                "user" => "User",
                "assistant" => "Assistant",
                _ => &msg.role,
            };
            let text = Self::extract_text_from_message(msg);
            if !text.is_empty() {
                parts.push(format!("{}: {}", role, text));
            }
        }
        parts.join("\n\n")
    }

    /// Extract only the last user message text (for --resume sessions).
    fn serialize_last_user_message(messages: &[Message]) -> String {
        messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| Self::extract_text_from_message(m))
            .unwrap_or_default()
    }

    /// Extract text content from a message's content blocks.
    fn extract_text_from_message(msg: &Message) -> String {
        msg.content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                ContentBlock::ToolResult { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Derive a conversation key from messages for session tracking.
    fn conversation_key(messages: &[Message]) -> String {
        // Use a hash of the first user message as a stable key.
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        if let Some(first) = messages.first() {
            Self::extract_text_from_message(first).hash(&mut hasher);
        }
        format!("conv_{:x}", hasher.finish())
    }

    /// Build the base command with common flags.
    /// If `system` is `None`, skip `--system-prompt` (e.g. on --resume).
    fn base_command(&self, system: Option<&str>, model: &str) -> Command {
        let mut cmd = Command::new(&self.claude_bin);
        if let Some(sys) = system {
            cmd.args(["--system-prompt", sys]);
        }
        cmd.args(["--model", model])
            .args(["--permission-mode", "bypassPermissions"]);
        cmd
    }

    /// Look up and validate a session for --resume.
    async fn get_session(&self, key: &str) -> Option<String> {
        let sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get(key) {
            let elapsed = session.last_used.elapsed().as_secs();
            if elapsed < self.session_ttl_secs {
                return Some(session.session_id.clone());
            }
        }
        None
    }

    /// Store/update a session after a successful CC call.
    async fn save_session(&self, key: &str, session_id: &str) {
        let mut sessions = self.sessions.lock().await;
        let now = std::time::Instant::now();
        if let Some(existing) = sessions.get_mut(key) {
            existing.session_id = session_id.to_string();
            existing.last_used = now;
            existing.turn_count += 1;
        } else {
            sessions.insert(
                key.to_string(),
                CcSession {
                    session_id: session_id.to_string(),
                    created_at: now,
                    last_used: now,
                    turn_count: 1,
                },
            );
        }

        // Prune expired sessions while we hold the lock.
        let ttl = self.session_ttl_secs;
        sessions.retain(|_, s| s.last_used.elapsed().as_secs() < ttl);
    }

    /// Run `claude -p` with --output-format json and return parsed response.
    async fn run_json(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        model: &str,
    ) -> Result<LlmResponse> {
        let prompt = Self::serialize_messages(messages);
        let mut cmd = self.base_command(Some(system), model);
        cmd.args(["-p", &prompt])
            .args(["--output-format", "json"]);

        if tools.is_empty() {
            cmd.args(["--allowedTools", ""])
                .args(["--max-turns", "1"]);
        } else {
            let cc_tools = Self::map_tools(tools);
            cmd.args(["--allowedTools", &cc_tools]);
            cmd.args(["--max-turns", &self.max_turns.to_string()]);
        }

        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        tracing::debug!("Spawning claude -p (json mode, model={})", model);

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(self.timeout_secs),
            cmd.output(),
        )
        .await
        .context("claude -p timed out")?
        .context("failed to spawn claude process")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let code = output.status.code().unwrap_or(-1);
            anyhow::bail!(
                "claude -p exited with code {}: {}",
                code,
                stderr.trim()
            );
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let result: CcJsonResult = serde_json::from_str(&stdout)
            .context("failed to parse claude -p JSON output")?;

        let usage = Usage {
            input_tokens: result.usage.input_tokens,
            output_tokens: result.usage.output_tokens,
            cache_read: result.usage.cache_read,
            cache_write: result.usage.cache_write,
        };

        // Track tokens.
        token_tracker().record(&usage);

        Ok(LlmResponse {
            text: Some(result.result),
            tool_calls: vec![], // CC handled tools internally.
            stop_reason: "end_turn".to_string(),
            usage,
        })
    }

    /// Map RustClaw ToolDefinition names to CC --allowedTools format.
    /// CC uses its own tool names (Read, Write, Edit, Bash, Glob, Grep, etc.)
    fn map_tools(tools: &[ToolDefinition]) -> String {
        tools
            .iter()
            .filter_map(|t| {
                // Map common RustClaw tool names to CC equivalents.
                match t.name.as_str() {
                    "read_file" => Some("Read"),
                    "write_file" => Some("Write"),
                    "edit_file" => Some("Edit"),
                    "bash" | "execute_command" => Some("Bash"),
                    "list_dir" | "glob" => Some("Glob"),
                    "grep" | "search" => Some("Grep"),
                    // Pass through names that already look like CC tools.
                    name if name.starts_with(|c: char| c.is_uppercase()) => Some(name),
                    _ => {
                        tracing::warn!("Unknown tool '{}' — not mapped to CC tool", t.name);
                        None
                    }
                }
            })
            .collect::<Vec<_>>()
            .join(",")
    }
}

#[async_trait::async_trait]
impl LlmClient for ClaudeCliClient {
    fn model_name(&self) -> &str {
        &self.model
    }

    fn clone_boxed(&self) -> Box<dyn LlmClient> {
        Box::new(Self {
            model: self.model.clone(),
            claude_bin: self.claude_bin.clone(),
            timeout_secs: self.timeout_secs,
            max_turns: self.max_turns,
            session_ttl_secs: self.session_ttl_secs,
            sessions: self.sessions.clone(),
        })
    }

    async fn chat(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> Result<LlmResponse> {
        self.run_json(system, messages, tools, &self.model.clone())
            .await
    }

    async fn chat_with_model(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        model_override: &str,
    ) -> Result<LlmResponse> {
        self.run_json(system, messages, tools, model_override).await
    }

    async fn chat_stream(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> Result<mpsc::Receiver<StreamChunk>> {
        let session_key = Self::conversation_key(messages);
        let existing_session = self.get_session(&session_key).await;

        // If resuming, only send the last user message; CC has the history.
        let prompt = if existing_session.is_some() {
            Self::serialize_last_user_message(messages)
        } else {
            Self::serialize_messages(messages)
        };

        // Don't pass --system-prompt on resume (CC already has it in session context).
        let sys_opt = if existing_session.is_some() {
            None
        } else {
            Some(system)
        };
        let mut cmd = self.base_command(sys_opt, &self.model);
        cmd.args(["-p", &prompt])
            .args(["--output-format", "stream-json"])
            .arg("--verbose");

        if let Some(ref sid) = existing_session {
            cmd.args(["--resume", sid]);
        }

        if tools.is_empty() {
            cmd.args(["--allowedTools", ""]);
        } else {
            let cc_tools = Self::map_tools(tools);
            cmd.args(["--allowedTools", &cc_tools]);
            cmd.args(["--max-turns", &self.max_turns.to_string()]);
        }

        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        tracing::debug!(
            "Spawning claude -p (stream mode, model={}, resume={})",
            self.model,
            existing_session.is_some()
        );

        let mut child = cmd.spawn().context("failed to spawn claude process")?;

        let stdout = child
            .stdout
            .take()
            .context("failed to capture claude stdout")?;
        let stderr = child
            .stderr
            .take()
            .context("failed to capture claude stderr")?;

        let (tx, rx) = mpsc::channel(100);
        let sessions = self.sessions.clone();
        let session_ttl = self.session_ttl_secs;
        let timeout_secs = self.timeout_secs;

        tokio::spawn(async move {
            let reader = tokio::io::BufReader::new(stdout);
            let mut lines = reader.lines();

            let result = tokio::time::timeout(
                std::time::Duration::from_secs(timeout_secs),
                async {
                    while let Ok(Some(line)) = lines.next_line().await {
                        if line.trim().is_empty() {
                            continue;
                        }

                        let event: CcStreamEvent = match serde_json::from_str(&line) {
                            Ok(e) => e,
                            Err(e) => {
                                tracing::warn!("Failed to parse CC stream event: {} — line: {}", e, &line[..line.len().min(200)]);
                                continue;
                            }
                        };

                        match event.type_.as_str() {
                            "assistant" => {
                                if let Some(msg) = &event.message {
                                    for block in &msg.content {
                                        match block {
                                            CcContentBlock::Text { text } => {
                                                if tx.send(StreamChunk::Text(text.clone())).await.is_err() {
                                                    return;
                                                }
                                            }
                                            CcContentBlock::ToolUse { id, name, input } => {
                                                let tool_call = ToolCall {
                                                    id: id.clone(),
                                                    name: name.clone(),
                                                    input: input.clone(),
                                                };
                                                if tx.send(StreamChunk::ToolUse(tool_call)).await.is_err() {
                                                    return;
                                                }
                                            }
                                            CcContentBlock::Other => {}
                                        }
                                    }
                                }
                            }
                            "result" => {
                                // Save session_id for future --resume.
                                if let Some(sid) = &event.session_id {
                                    if !sid.is_empty() {
                                        let mut sess = sessions.lock().await;
                                        let now = std::time::Instant::now();
                                        sess.insert(
                                            session_key.clone(),
                                            CcSession {
                                                session_id: sid.clone(),
                                                created_at: now,
                                                last_used: now,
                                                turn_count: 1,
                                            },
                                        );
                                        // Prune expired.
                                        sess.retain(|_, s| s.last_used.elapsed().as_secs() < session_ttl);
                                    }
                                }

                                let usage = event.usage.unwrap_or_default();
                                let usage = Usage {
                                    input_tokens: usage.input_tokens,
                                    output_tokens: usage.output_tokens,
                                    cache_read: usage.cache_read,
                                    cache_write: usage.cache_write,
                                };
                                token_tracker().record(&usage);

                                let stop_reason = match event.subtype.as_deref() {
                                    Some("success") => "end_turn",
                                    Some(other) => other,
                                    None => "end_turn",
                                };
                                let _ = tx
                                    .send(StreamChunk::Done(usage, stop_reason.to_string()))
                                    .await;
                                return;
                            }
                            // "system" (init), "rate_limit_event", etc. — ignore.
                            _ => {}
                        }
                    }

                    // Stream ended without a "result" event — likely a crash.
                    let status = child.wait().await;
                    let mut stderr_buf = String::new();
                    let mut stderr_reader = tokio::io::BufReader::new(stderr);
                    let _ = tokio::io::AsyncReadExt::read_to_string(&mut stderr_reader, &mut stderr_buf).await;

                    let err_msg = if stderr_buf.trim().is_empty() {
                        format!(
                            "claude -p stream ended unexpectedly (exit: {:?})",
                            status.as_ref().ok().map(|s| s.code())
                        )
                    } else {
                        format!(
                            "claude -p error (exit: {:?}): {}",
                            status.as_ref().ok().map(|s| s.code()),
                            stderr_buf.trim()
                        )
                    };
                    tracing::error!("{}", err_msg);

                    // Send a Done with zero usage so the caller doesn't hang.
                    let _ = tx
                        .send(StreamChunk::Done(Usage::default(), "error".to_string()))
                        .await;
                },
            )
            .await;

            if result.is_err() {
                tracing::error!("claude -p stream timed out");
                let _ = child.kill().await;
                let _ = tx
                    .send(StreamChunk::Done(Usage::default(), "timeout".to_string()))
                    .await;
            }
        });

        Ok(rx)
    }
}
