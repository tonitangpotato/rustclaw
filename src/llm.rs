//! LLM provider abstraction.
//!
//! Supports Anthropic (Claude) natively, with extensibility for OpenAI and others.

use serde::{Deserialize, Serialize};

use crate::config::{self, LlmConfig};

/// A message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
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
    pub content: Option<String>,
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

/// Anthropic Claude client.
pub struct AnthropicClient {
    client: reqwest::Client,
    api_key: String,
    model: String,
    max_tokens: u32,
    base_url: String,
}

impl AnthropicClient {
    pub fn new(config: &LlmConfig) -> anyhow::Result<Self> {
        let api_key = config::resolve_api_key(config)?;
        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.anthropic.com".to_string());

        Ok(Self {
            client: reqwest::Client::new(),
            api_key,
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
            "messages": messages.iter().map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content,
                })
            }).collect::<Vec<_>>(),
        });

        if !tools.is_empty() {
            body["tools"] = serde_json::json!(tools.iter().map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            }).collect::<Vec<_>>());
        }

        let resp = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let resp_body: serde_json::Value = resp.json().await?;

        if !status.is_success() {
            let error_msg = resp_body["error"]["message"]
                .as_str()
                .unwrap_or("Unknown error");
            anyhow::bail!("Anthropic API error ({}): {}", status, error_msg);
        }

        // Parse response
        let mut content = None;
        let mut tool_calls = Vec::new();

        if let Some(content_blocks) = resp_body["content"].as_array() {
            for block in content_blocks {
                match block["type"].as_str() {
                    Some("text") => {
                        content = block["text"].as_str().map(String::from);
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
        }

        let usage = Usage {
            input_tokens: resp_body["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
            output_tokens: resp_body["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
            cache_read: resp_body["usage"]["cache_creation_input_tokens"]
                .as_u64()
                .unwrap_or(0) as u32,
            cache_write: resp_body["usage"]["cache_read_input_tokens"]
                .as_u64()
                .unwrap_or(0) as u32,
        };

        Ok(LlmResponse {
            content,
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
