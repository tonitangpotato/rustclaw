//! LLM provider abstraction.
//!
//! Supports Anthropic (Claude) natively, with extensibility for OpenAI and others.
//! Uses proper Anthropic Messages API content blocks for tool_use/tool_result.

use serde::{Deserialize, Serialize};

use crate::config::{self, AuthMode, LlmConfig};

/// A content block in a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        is_error: bool,
    },
}

/// A message in the conversation (with proper content blocks).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Vec<ContentBlock>,
}

impl Message {
    /// Create a simple text message.
    pub fn text(role: &str, text: &str) -> Self {
        Self {
            role: role.to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
        }
    }

    /// Create an assistant message with tool use blocks.
    pub fn assistant_with_tools(text: Option<&str>, tool_calls: Vec<ToolCall>) -> Self {
        let mut content = Vec::new();
        if let Some(t) = text {
            content.push(ContentBlock::Text {
                text: t.to_string(),
            });
        }
        for tc in tool_calls {
            content.push(ContentBlock::ToolUse {
                id: tc.id,
                name: tc.name,
                input: tc.input,
            });
        }
        Self {
            role: "assistant".to_string(),
            content,
        }
    }

    /// Create a user message with tool results.
    pub fn tool_results(results: Vec<(String, String, bool)>) -> Self {
        Self {
            role: "user".to_string(),
            content: results
                .into_iter()
                .map(|(id, output, is_error)| ContentBlock::ToolResult {
                    tool_use_id: id,
                    content: output,
                    is_error,
                })
                .collect(),
        }
    }
}

/// A tool definition for the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// A tool call from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

/// LLM response.
#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub text: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub stop_reason: String,
    pub usage: Usage,
}

/// Token usage.
#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read: u32,
    pub cache_write: u32,
}

/// LLM client trait.
#[async_trait::async_trait]
pub trait LlmClient: Send + Sync {
    async fn chat(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> anyhow::Result<LlmResponse>;
}

/// Anthropic Claude client (supports both API key and OAuth token).
pub struct AnthropicClient {
    client: reqwest::Client,
    auth: AuthMode,
    model: String,
    max_tokens: u32,
    base_url: String,
}

impl AnthropicClient {
    pub fn new(config: &LlmConfig) -> anyhow::Result<Self> {
        let auth = config::resolve_auth(config)?;
        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.anthropic.com".to_string());

        Ok(Self {
            client: reqwest::Client::new(),
            auth,
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            base_url,
        })
    }
}

#[async_trait::async_trait]
impl LlmClient for AnthropicClient {
    async fn chat(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> anyhow::Result<LlmResponse> {
        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "system": system,
            "messages": serde_json::to_value(messages)?,
        });

        if !tools.is_empty() {
            body["tools"] = serde_json::json!(tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.name,
                        "description": t.description,
                        "input_schema": t.input_schema,
                    })
                })
                .collect::<Vec<_>>());
        }

        let mut req = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json");

        // Auth headers differ between API key and OAuth token
        match &self.auth {
            AuthMode::ApiKey(key) => {
                req = req.header("x-api-key", key);
            }
            AuthMode::OAuthToken(token) => {
                req = req
                    .header("Authorization", format!("Bearer {}", token))
                    .header(
                        "anthropic-beta",
                        "claude-code-20250219,oauth-2025-04-20",
                    )
                    .header("user-agent", "claude-cli/2.1.39 (external, cli)")
                    .header("x-app", "cli")
                    .header("anthropic-dangerous-direct-browser-access", "true");
            }
        }

        let resp = req.json(&body).send().await?;

        let status = resp.status();
        let resp_body: serde_json::Value = resp.json().await?;

        if !status.is_success() {
            let error_msg = resp_body["error"]["message"]
                .as_str()
                .unwrap_or("Unknown error");
            anyhow::bail!("Anthropic API error ({}): {}", status, error_msg);
        }

        // Parse response content blocks
        let mut text = None;
        let mut tool_calls = Vec::new();

        if let Some(content_blocks) = resp_body["content"].as_array() {
            let mut text_parts = Vec::new();
            for block in content_blocks {
                match block["type"].as_str() {
                    Some("text") => {
                        if let Some(t) = block["text"].as_str() {
                            text_parts.push(t.to_string());
                        }
                    }
                    Some("tool_use") => {
                        tool_calls.push(ToolCall {
                            id: block["id"].as_str().unwrap_or("").to_string(),
                            name: block["name"].as_str().unwrap_or("").to_string(),
                            input: block["input"].clone(),
                        });
                    }
                    _ => {}
                }
            }
            if !text_parts.is_empty() {
                text = Some(text_parts.join("\n"));
            }
        }

        let usage = Usage {
            input_tokens: resp_body["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
            output_tokens: resp_body["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
            cache_read: resp_body["usage"]["cache_read_input_tokens"]
                .as_u64()
                .unwrap_or(0) as u32,
            cache_write: resp_body["usage"]["cache_creation_input_tokens"]
                .as_u64()
                .unwrap_or(0) as u32,
        };

        Ok(LlmResponse {
            text,
            tool_calls,
            stop_reason: resp_body["stop_reason"]
                .as_str()
                .unwrap_or("unknown")
                .to_string(),
            usage,
        })
    }
}

/// Create an LLM client based on config.
pub fn create_client(config: &LlmConfig) -> anyhow::Result<Box<dyn LlmClient>> {
    match config.provider.as_str() {
        "anthropic" => Ok(Box::new(AnthropicClient::new(config)?)),
        other => anyhow::bail!("Unsupported LLM provider: {}", other),
    }
}
