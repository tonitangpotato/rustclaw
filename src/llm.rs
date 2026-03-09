//! LLM provider abstraction.
//!
//! Supports Anthropic (Claude) natively, with extensibility for OpenAI and others.
//! Uses proper Anthropic Messages API content blocks for tool_use/tool_result.
//! Includes streaming support via SSE for real-time responses.

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

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

/// A chunk from streaming response.
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// Partial text content
    Text(String),
    /// Complete tool use block
    ToolUse(ToolCall),
    /// Stream finished with final usage stats
    Done(Usage, String), // (usage, stop_reason)
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

    /// Stream chat response, sending chunks through the channel.
    /// Returns immediately, chunks arrive via the returned receiver.
    async fn chat_stream(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> anyhow::Result<mpsc::Receiver<StreamChunk>>;
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
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()?,
            auth,
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            base_url,
        })
    }
}

/// Retry configuration
const MAX_RETRIES: u32 = 5;
const INITIAL_BACKOFF_MS: u64 = 1000;

/// Check if a status code should trigger a retry.
fn should_retry(status: reqwest::StatusCode) -> bool {
    matches!(
        status.as_u16(),
        429 | 500 | 502 | 503 | 529
    )
}

/// Check if a status code is a client error (should NOT retry).
fn is_client_error(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 400 | 401 | 403 | 404)
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

        // Retry loop with exponential backoff
        let mut attempt = 0;
        let mut last_error: Option<anyhow::Error> = None;

        loop {
            attempt += 1;

            tracing::info!(
                "LLM request attempt {}/{} → model={} url={}/v1/messages auth={}",
                attempt, MAX_RETRIES, self.model, self.base_url,
                match &self.auth { AuthMode::OAuthToken(_) => "oauth", AuthMode::ApiKey(_) => "api_key" }
            );

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

            let resp = match req.json(&body).send().await {
                Ok(r) => r,
                Err(e) => {
                    if attempt <= MAX_RETRIES {
                        let backoff = INITIAL_BACKOFF_MS * 2u64.pow(attempt - 1);
                        tracing::warn!(
                            "Request failed (attempt {}/{}): {}. Retrying in {}ms...",
                            attempt, MAX_RETRIES, e, backoff
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
                        last_error = Some(e.into());
                        continue;
                    }
                    return Err(e.into());
                }
            };

            let status = resp.status();

            // Check for client errors - don't retry these
            if is_client_error(status) {
                let resp_body: serde_json::Value = resp.json().await?;
                let error_msg = resp_body["error"]["message"]
                    .as_str()
                    .unwrap_or("Unknown error");
                anyhow::bail!("Anthropic API error ({}): {}", status, error_msg);
            }

            // Check for retryable errors
            if should_retry(status) && attempt <= MAX_RETRIES {
                // Check for retry-after header (for 429)
                let retry_after = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok());

                let backoff = retry_after
                    .map(|secs| secs * 1000)
                    .unwrap_or_else(|| INITIAL_BACKOFF_MS * 2u64.pow(attempt - 1));

                tracing::warn!(
                    "Retryable error {} (attempt {}/{}). Retrying in {}ms...",
                    status, attempt, MAX_RETRIES, backoff
                );

                tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
                last_error = Some(anyhow::anyhow!("HTTP {}", status));
                continue;
            }

            // Non-retryable error or success
            let resp_body: serde_json::Value = resp.json().await?;

            if !status.is_success() {
                let error_msg = resp_body["error"]["message"]
                    .as_str()
                    .unwrap_or("Unknown error");

                // If we've exhausted retries, include last error info
                if let Some(le) = &last_error {
                    anyhow::bail!(
                        "Anthropic API error ({}) after {} attempts: {} (last error: {})",
                        status, attempt, error_msg, le
                    );
                }
                anyhow::bail!("Anthropic API error ({}): {}", status, error_msg);
            }

            // Success! Parse response content blocks
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

            return Ok(LlmResponse {
                text,
                tool_calls,
                stop_reason: resp_body["stop_reason"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
                usage,
            });
        } // end retry loop
    }

    async fn chat_stream(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> anyhow::Result<mpsc::Receiver<StreamChunk>> {
        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "system": system,
            "messages": serde_json::to_value(messages)?,
            "stream": true,
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

        if !status.is_success() {
            let error_body: serde_json::Value = resp.json().await?;
            let error_msg = error_body["error"]["message"]
                .as_str()
                .unwrap_or("Unknown error");
            anyhow::bail!("Anthropic API error ({}): {}", status, error_msg);
        }

        let (tx, rx) = mpsc::channel::<StreamChunk>(100);

        // Spawn task to process SSE stream
        let byte_stream = resp.bytes_stream();
        tokio::spawn(async move {
            let mut stream = byte_stream;
            let mut buffer = String::new();
            let mut current_tool: Option<PartialToolUse> = None;
            let mut usage = Usage::default();
            let mut stop_reason = String::new();

            while let Some(chunk_result) = stream.next().await {
                let chunk = match chunk_result {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!("Stream error: {}", e);
                        break;
                    }
                };

                buffer.push_str(&String::from_utf8_lossy(&chunk));

                // Process complete SSE events (lines starting with "data: ")
                while let Some(event) = extract_sse_event(&mut buffer) {
                    if event == "[DONE]" {
                        break;
                    }

                    if let Ok(data) = serde_json::from_str::<serde_json::Value>(&event) {
                        match data["type"].as_str() {
                            Some("content_block_start") => {
                                // Check if it's a tool_use block
                                if data["content_block"]["type"].as_str() == Some("tool_use") {
                                    current_tool = Some(PartialToolUse {
                                        id: data["content_block"]["id"]
                                            .as_str()
                                            .unwrap_or("")
                                            .to_string(),
                                        name: data["content_block"]["name"]
                                            .as_str()
                                            .unwrap_or("")
                                            .to_string(),
                                        input_json: String::new(),
                                    });
                                }
                            }
                            Some("content_block_delta") => {
                                if let Some(delta) = data.get("delta") {
                                    match delta["type"].as_str() {
                                        Some("text_delta") => {
                                            if let Some(text) = delta["text"].as_str() {
                                                let _ = tx.send(StreamChunk::Text(text.to_string())).await;
                                            }
                                        }
                                        Some("input_json_delta") => {
                                            if let Some(partial) = delta["partial_json"].as_str() {
                                                if let Some(ref mut tool) = current_tool {
                                                    tool.input_json.push_str(partial);
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            Some("content_block_stop") => {
                                // If we were building a tool call, emit it
                                if let Some(tool) = current_tool.take() {
                                    let input: serde_json::Value =
                                        serde_json::from_str(&tool.input_json)
                                            .unwrap_or(serde_json::json!({}));
                                    let _ = tx
                                        .send(StreamChunk::ToolUse(ToolCall {
                                            id: tool.id,
                                            name: tool.name,
                                            input,
                                        }))
                                        .await;
                                }
                            }
                            Some("message_delta") => {
                                if let Some(sr) = data["delta"]["stop_reason"].as_str() {
                                    stop_reason = sr.to_string();
                                }
                                if let Some(u) = data.get("usage") {
                                    usage.output_tokens =
                                        u["output_tokens"].as_u64().unwrap_or(0) as u32;
                                }
                            }
                            Some("message_start") => {
                                if let Some(u) = data["message"].get("usage") {
                                    usage.input_tokens =
                                        u["input_tokens"].as_u64().unwrap_or(0) as u32;
                                    usage.cache_read =
                                        u["cache_read_input_tokens"].as_u64().unwrap_or(0) as u32;
                                    usage.cache_write =
                                        u["cache_creation_input_tokens"].as_u64().unwrap_or(0) as u32;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            // Send final Done chunk
            let _ = tx.send(StreamChunk::Done(usage, stop_reason)).await;
        });

        Ok(rx)
    }
}

/// Partial tool use being accumulated during streaming.
struct PartialToolUse {
    id: String,
    name: String,
    input_json: String,
}

/// Extract a complete SSE event from the buffer.
/// Returns the data portion (after "data: ") if a complete event is found.
fn extract_sse_event(buffer: &mut String) -> Option<String> {
    // SSE events are separated by double newlines
    // Each line within an event starts with "data: " for data lines
    if let Some(pos) = buffer.find("\n\n") {
        let event = buffer[..pos].to_string();
        *buffer = buffer[pos + 2..].to_string();

        // Extract data from "data: " prefix
        for line in event.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                return Some(data.to_string());
            }
        }
    }
    None
}

// ─── OpenAI Client ───────────────────────────────────────────

/// OpenAI API client.
pub struct OpenAIClient {
    client: reqwest::Client,
    api_key: String,
    model: String,
    max_tokens: u32,
    base_url: String,
}

impl OpenAIClient {
    pub fn new(config: &LlmConfig) -> anyhow::Result<Self> {
        let api_key = config
            .api_key
            .clone()
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .ok_or_else(|| anyhow::anyhow!("OpenAI API key not found"))?;

        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com".to_string());

        Ok(Self {
            client: reqwest::Client::new(),
            api_key,
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            base_url,
        })
    }

    /// Convert internal messages to OpenAI format.
    fn convert_messages(&self, system: &str, messages: &[Message]) -> Vec<serde_json::Value> {
        let mut result = vec![serde_json::json!({
            "role": "system",
            "content": system
        })];

        for msg in messages {
            // Extract text content from content blocks
            let content: Vec<serde_json::Value> = msg
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(serde_json::json!({
                        "type": "text",
                        "text": text
                    })),
                    ContentBlock::ToolUse { id, name, input } => Some(serde_json::json!({
                        "type": "function",
                        "id": id,
                        "function": {
                            "name": name,
                            "arguments": input.to_string()
                        }
                    })),
                    ContentBlock::ToolResult { tool_use_id, content, .. } => Some(serde_json::json!({
                        "type": "tool_result",
                        "tool_call_id": tool_use_id,
                        "content": content
                    })),
                })
                .collect();

            // For simple text messages, just use string content
            if content.len() == 1 {
                if let Some(text) = content[0].get("text") {
                    result.push(serde_json::json!({
                        "role": msg.role,
                        "content": text
                    }));
                    continue;
                }
            }

            result.push(serde_json::json!({
                "role": msg.role,
                "content": content
            }));
        }

        result
    }

    /// Convert internal tools to OpenAI format.
    fn convert_tools(&self, tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
        tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema
                    }
                })
            })
            .collect()
    }
}

#[async_trait::async_trait]
impl LlmClient for OpenAIClient {
    async fn chat(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> anyhow::Result<LlmResponse> {
        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": self.convert_messages(system, messages),
        });

        if !tools.is_empty() {
            body["tools"] = serde_json::json!(self.convert_tools(tools));
        }

        let resp = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let resp_body: serde_json::Value = resp.json().await?;

        if !status.is_success() {
            let error_msg = resp_body["error"]["message"]
                .as_str()
                .unwrap_or("Unknown error");
            anyhow::bail!("OpenAI API error ({}): {}", status, error_msg);
        }

        // Parse response
        let choice = &resp_body["choices"][0];
        let msg = &choice["message"];

        let text = msg["content"].as_str().map(|s| s.to_string());

        let mut tool_calls = Vec::new();
        if let Some(calls) = msg["tool_calls"].as_array() {
            for call in calls {
                let func = &call["function"];
                tool_calls.push(ToolCall {
                    id: call["id"].as_str().unwrap_or("").to_string(),
                    name: func["name"].as_str().unwrap_or("").to_string(),
                    input: serde_json::from_str(func["arguments"].as_str().unwrap_or("{}"))
                        .unwrap_or(serde_json::json!({})),
                });
            }
        }

        let usage = Usage {
            input_tokens: resp_body["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            output_tokens: resp_body["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32,
            cache_read: 0,
            cache_write: 0,
        };

        Ok(LlmResponse {
            text,
            tool_calls,
            stop_reason: choice["finish_reason"]
                .as_str()
                .unwrap_or("unknown")
                .to_string(),
            usage,
        })
    }

    async fn chat_stream(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> anyhow::Result<tokio::sync::mpsc::Receiver<StreamChunk>> {
        // For now, just do non-streaming and convert to stream format
        let response = self.chat(system, messages, tools).await?;

        let (tx, rx) = tokio::sync::mpsc::channel(10);

        tokio::spawn(async move {
            if let Some(text) = response.text {
                let _ = tx.send(StreamChunk::Text(text)).await;
            }
            for tc in response.tool_calls {
                let _ = tx.send(StreamChunk::ToolUse(tc)).await;
            }
            let _ = tx.send(StreamChunk::Done(response.usage, response.stop_reason)).await;
        });

        Ok(rx)
    }
}

// ─── Google Client ───────────────────────────────────────────

/// Google Generative AI client.
pub struct GoogleClient {
    client: reqwest::Client,
    api_key: String,
    model: String,
    max_tokens: u32,
}

impl GoogleClient {
    pub fn new(config: &LlmConfig) -> anyhow::Result<Self> {
        let api_key = config
            .api_key
            .clone()
            .or_else(|| std::env::var("GOOGLE_API_KEY").ok())
            .ok_or_else(|| anyhow::anyhow!("Google API key not found"))?;

        Ok(Self {
            client: reqwest::Client::new(),
            api_key,
            model: config.model.clone(),
            max_tokens: config.max_tokens,
        })
    }

    /// Convert messages to Google format.
    fn convert_messages(&self, system: &str, messages: &[Message]) -> (String, Vec<serde_json::Value>) {
        let mut contents = Vec::new();

        for msg in messages {
            let role = match msg.role.as_str() {
                "user" => "user",
                "assistant" => "model",
                _ => "user",
            };

            let parts: Vec<serde_json::Value> = msg
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(serde_json::json!({ "text": text })),
                    _ => None,
                })
                .collect();

            if !parts.is_empty() {
                contents.push(serde_json::json!({
                    "role": role,
                    "parts": parts
                }));
            }
        }

        (system.to_string(), contents)
    }

    /// Convert tools to Google format.
    fn convert_tools(&self, tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
        if tools.is_empty() {
            return Vec::new();
        }

        let function_declarations: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema
                })
            })
            .collect();

        vec![serde_json::json!({
            "function_declarations": function_declarations
        })]
    }
}

#[async_trait::async_trait]
impl LlmClient for GoogleClient {
    async fn chat(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> anyhow::Result<LlmResponse> {
        let (system_instruction, contents) = self.convert_messages(system, messages);

        let mut body = serde_json::json!({
            "system_instruction": {
                "parts": [{ "text": system_instruction }]
            },
            "contents": contents,
            "generationConfig": {
                "maxOutputTokens": self.max_tokens
            }
        });

        let converted_tools = self.convert_tools(tools);
        if !converted_tools.is_empty() {
            body["tools"] = serde_json::json!(converted_tools);
        }

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let resp_body: serde_json::Value = resp.json().await?;

        if !status.is_success() {
            let error_msg = resp_body["error"]["message"]
                .as_str()
                .unwrap_or("Unknown error");
            anyhow::bail!("Google API error ({}): {}", status, error_msg);
        }

        // Parse response
        let candidate = &resp_body["candidates"][0];
        let content = &candidate["content"];

        let mut text = None;
        let mut tool_calls = Vec::new();

        if let Some(parts) = content["parts"].as_array() {
            for part in parts {
                if let Some(t) = part["text"].as_str() {
                    text = Some(t.to_string());
                }
                if let Some(fc) = part.get("functionCall") {
                    tool_calls.push(ToolCall {
                        id: uuid::Uuid::new_v4().to_string(),
                        name: fc["name"].as_str().unwrap_or("").to_string(),
                        input: fc["args"].clone(),
                    });
                }
            }
        }

        let usage = Usage {
            input_tokens: resp_body["usageMetadata"]["promptTokenCount"]
                .as_u64()
                .unwrap_or(0) as u32,
            output_tokens: resp_body["usageMetadata"]["candidatesTokenCount"]
                .as_u64()
                .unwrap_or(0) as u32,
            cache_read: 0,
            cache_write: 0,
        };

        Ok(LlmResponse {
            text,
            tool_calls,
            stop_reason: candidate["finishReason"]
                .as_str()
                .unwrap_or("unknown")
                .to_string(),
            usage,
        })
    }

    async fn chat_stream(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> anyhow::Result<tokio::sync::mpsc::Receiver<StreamChunk>> {
        // For now, just do non-streaming
        let response = self.chat(system, messages, tools).await?;

        let (tx, rx) = tokio::sync::mpsc::channel(10);

        tokio::spawn(async move {
            if let Some(text) = response.text {
                let _ = tx.send(StreamChunk::Text(text)).await;
            }
            for tc in response.tool_calls {
                let _ = tx.send(StreamChunk::ToolUse(tc)).await;
            }
            let _ = tx.send(StreamChunk::Done(response.usage, response.stop_reason)).await;
        });

        Ok(rx)
    }
}

/// Create an LLM client based on config.
pub fn create_client(config: &LlmConfig) -> anyhow::Result<Box<dyn LlmClient>> {
    match config.provider.as_str() {
        "anthropic" => Ok(Box::new(AnthropicClient::new(config)?)),
        "openai" => Ok(Box::new(OpenAIClient::new(config)?)),
        "google" => Ok(Box::new(GoogleClient::new(config)?)),
        other => anyhow::bail!("Unsupported LLM provider: {}. Supported: anthropic, openai, google", other),
    }
}
