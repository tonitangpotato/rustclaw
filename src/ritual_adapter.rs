//! Adapter: wraps RustClaw's LlmClient to implement gid-core's ritual LlmClient trait.
//!
//! This gives ritual Skill phases RustClaw's full auth stack:
//! OAuth refresh, 11-retry exponential backoff, rate limit handling.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;
use gid_core::ritual::llm::{LlmClient as GidLlmClient, ToolDefinition, SkillResult};
use crate::llm::{self, LlmClient, Message, ContentBlock};

/// Adapter bridging RustClaw's LlmClient → gid-core's ritual LlmClient.
pub struct RitualLlmAdapter {
    client: Arc<tokio::sync::RwLock<Box<dyn LlmClient>>>,
}

impl RitualLlmAdapter {
    pub fn new(client: Arc<tokio::sync::RwLock<Box<dyn LlmClient>>>) -> Self {
        Self { client }
    }

    pub fn into_arc(self) -> Arc<dyn GidLlmClient> {
        Arc::new(self)
    }
}

#[async_trait]
impl GidLlmClient for RitualLlmAdapter {
    async fn run_skill(
        &self,
        skill_prompt: &str,
        tools: Vec<ToolDefinition>,
        model: &str,
        working_dir: &Path,
    ) -> Result<SkillResult> {
        // Resolve model aliases
        let resolved_model = match model {
            "sonnet" => "claude-sonnet-4-5-20250929",
            "opus" => "claude-opus-4-6",
            "haiku" => "claude-haiku-3-5-20241022",
            other => other,
        };

        // Convert gid-core ToolDefinition → RustClaw ToolDefinition
        let rc_tools: Vec<llm::ToolDefinition> = tools.iter().map(|t| {
            llm::ToolDefinition {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: t.input_schema.clone(),
            }
        }).collect();

        let system = format!(
            "You are a development assistant executing a ritual phase.\n\
             Working directory: {}\n\n\
             Complete the task and produce the required artifacts.\n\n\
             {}",
            working_dir.display(),
            skill_prompt
        );

        let mut messages = vec![
            Message::text("user", "Execute the skill described in your system prompt. Use the provided tools to read files, write files, and run commands as needed.")
        ];

        let mut total_tool_calls = 0usize;
        let mut total_tokens = 0u64;
        let mut final_text = String::new();
        let handler = SkillToolHandler { working_dir: working_dir.to_path_buf() };

        // Mini agent loop — up to 20 turns
        for _turn in 0..20 {
            let response = {
                let client = self.client.read().await;
                client.chat(
                    &system,
                    &messages,
                    &rc_tools,
                ).await?
            };

            total_tokens += (response.usage.input_tokens + response.usage.output_tokens) as u64;

            // Collect text
            if let Some(ref text) = response.text {
                final_text = text.clone();
            }

            // If no tool calls, we're done
            if response.tool_calls.is_empty() {
                break;
            }

            // Build assistant message with tool calls
            let mut assistant_content = Vec::new();
            if let Some(ref text) = response.text {
                if !text.is_empty() {
                    assistant_content.push(ContentBlock::Text { text: text.clone() });
                }
            }
            for tc in &response.tool_calls {
                assistant_content.push(ContentBlock::ToolUse {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    input: tc.input.clone(),
                });
            }
            messages.push(Message { role: "assistant".to_string(), content: assistant_content });

            // Execute tools and build results
            let mut tool_results = Vec::new();
            for tc in &response.tool_calls {
                total_tool_calls += 1;
                let result = handler.handle(&tc.name, &tc.input).await;
                let (content, is_error) = match result {
                    Ok(output) => (output, false),
                    Err(e) => (format!("Error: {}", e), true),
                };
                tool_results.push(ContentBlock::ToolResult {
                    tool_use_id: tc.id.clone(),
                    content,
                    is_error,
                });
            }
            messages.push(Message { role: "user".to_string(), content: tool_results });
        }

        Ok(SkillResult {
            output: final_text,
            artifacts_created: vec![],
            tool_calls_made: total_tool_calls,
            tokens_used: total_tokens,
        })
    }
}

/// Tool handler for Read/Write/Bash within a skill phase.
struct SkillToolHandler {
    working_dir: PathBuf,
}

impl SkillToolHandler {
    async fn handle(&self, name: &str, input: &serde_json::Value) -> Result<String> {
        match name {
            "Read" => {
                let path = input.get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing 'path'"))?;
                let full = self.working_dir.join(path);
                let content = tokio::fs::read_to_string(&full).await
                    .map_err(|e| anyhow::anyhow!("Read {}: {}", full.display(), e))?;
                // Truncate to 50k
                Ok(if content.len() > 50_000 { content[..50_000].to_string() } else { content })
            }
            "Write" => {
                let path = input.get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing 'path'"))?;
                let content = input.get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing 'content'"))?;
                let full = self.working_dir.join(path);
                if let Some(parent) = full.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::write(&full, content).await?;
                Ok(format!("Written {} bytes to {}", content.len(), path))
            }
            "Bash" => {
                let command = input.get("command")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing 'command'"))?;
                let output = tokio::process::Command::new("bash")
                    .arg("-c")
                    .arg(command)
                    .current_dir(&self.working_dir)
                    .output()
                    .await?;
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let mut result = String::new();
                if !stdout.is_empty() { result.push_str(&stdout); }
                if !stderr.is_empty() { result.push_str("\nSTDERR: "); result.push_str(&stderr); }
                // Truncate to 20k
                Ok(if result.len() > 20_000 { result[..20_000].to_string() } else { result })
            }
            other => Err(anyhow::anyhow!("Unknown tool: {}", other)),
        }
    }
}
