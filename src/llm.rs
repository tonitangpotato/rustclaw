//! LLM provider abstraction.
//!
//! Supports Anthropic (Claude) natively, with extensibility for OpenAI and others.
//! Uses proper Anthropic Messages API content blocks for tool_use/tool_result.
//! Includes streaming support via SSE for real-time responses.

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use crate::auth_profiles::{AuthProfileCredential, AuthProfileFailureReason, AuthProfileManager};
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

/// Token tracking for cumulative usage across sessions.
/// Thread-safe with atomic operations.
pub struct TokenTracker {
    /// Total input tokens across all requests
    total_input: std::sync::atomic::AtomicU64,
    /// Total output tokens across all requests
    total_output: std::sync::atomic::AtomicU64,
    /// Total requests made
    total_requests: std::sync::atomic::AtomicU64,
    /// Total cache read tokens
    total_cache_read: std::sync::atomic::AtomicU64,
    /// Total cache write tokens
    total_cache_write: std::sync::atomic::AtomicU64,
    /// Sliding window: recent token records for rate tracking
    window: std::sync::Mutex<TokenWindow>,
    /// Alert callback (set once at startup)
    alert_fn: std::sync::OnceLock<Box<dyn Fn(TokenAlert) + Send + Sync>>,
}

/// A time-bucketed record for sliding window tracking.
#[derive(Debug, Clone)]
struct WindowEntry {
    timestamp: std::time::Instant,
    input_tokens: u64,
    output_tokens: u64,
}

/// Sliding window for rate-based alerts.
#[derive(Debug)]
struct TokenWindow {
    entries: Vec<WindowEntry>,
    /// Hourly alert threshold (total input+output)
    hourly_limit: u64,
    /// Whether we've already alerted for this window (avoid spam)
    alerted_this_hour: bool,
    /// Last time we pruned old entries
    last_prune: std::time::Instant,
}

/// Alert emitted when token usage exceeds threshold.
#[derive(Debug, Clone)]
pub struct TokenAlert {
    pub hourly_tokens: u64,
    pub hourly_limit: u64,
    pub hourly_requests: u64,
    pub total_tokens: u64,
    pub message: String,
}

impl TokenTracker {
    /// Create a new token tracker.
    pub fn new() -> Self {
        Self {
            total_input: std::sync::atomic::AtomicU64::new(0),
            total_output: std::sync::atomic::AtomicU64::new(0),
            total_requests: std::sync::atomic::AtomicU64::new(0),
            total_cache_read: std::sync::atomic::AtomicU64::new(0),
            total_cache_write: std::sync::atomic::AtomicU64::new(0),
            window: std::sync::Mutex::new(TokenWindow {
                entries: Vec::new(),
                hourly_limit: 2_000_000, // Default: 2M tokens/hour
                alerted_this_hour: false,
                last_prune: std::time::Instant::now(),
            }),
            alert_fn: std::sync::OnceLock::new(),
        }
    }

    /// Set the hourly token limit for alerts.
    pub fn set_hourly_limit(&self, limit: u64) {
        if let Ok(mut window) = self.window.lock() {
            window.hourly_limit = limit;
        }
    }

    /// Set the alert callback (called when hourly limit is exceeded).
    pub fn set_alert_fn(&self, f: impl Fn(TokenAlert) + Send + Sync + 'static) {
        let _ = self.alert_fn.set(Box::new(f));
    }

    /// Record token usage from a request.
    pub fn record(&self, usage: &Usage) {
        use std::sync::atomic::Ordering;
        self.total_input.fetch_add(usage.input_tokens as u64, Ordering::Relaxed);
        self.total_output.fetch_add(usage.output_tokens as u64, Ordering::Relaxed);
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.total_cache_read.fetch_add(usage.cache_read as u64, Ordering::Relaxed);
        self.total_cache_write.fetch_add(usage.cache_write as u64, Ordering::Relaxed);

        let total = self.total_input.load(Ordering::Relaxed) + self.total_output.load(Ordering::Relaxed);

        tracing::debug!(
            "Token usage: input={} output={} cache_read={} cache_write={} (cumulative: {} requests, {} total tokens)",
            usage.input_tokens, usage.output_tokens, usage.cache_read, usage.cache_write,
            self.total_requests.load(Ordering::Relaxed),
            total,
        );

        // Sliding window check
        if let Ok(mut window) = self.window.lock() {
            let now = std::time::Instant::now();

            // Add entry
            window.entries.push(WindowEntry {
                timestamp: now,
                input_tokens: usage.input_tokens as u64,
                output_tokens: usage.output_tokens as u64,
            });

            // Prune entries older than 1 hour (but not too frequently)
            if now.duration_since(window.last_prune).as_secs() > 60 {
                let one_hour_ago = now - std::time::Duration::from_secs(3600);
                window.entries.retain(|e| e.timestamp >= one_hour_ago);
                window.last_prune = now;

                // Reset alert flag if we're in a new window
                let hourly_total: u64 = window.entries.iter()
                    .map(|e| e.input_tokens + e.output_tokens)
                    .sum();
                if hourly_total < window.hourly_limit / 2 {
                    window.alerted_this_hour = false;
                }
            }

            // Check hourly rate
            let one_hour_ago = now - std::time::Duration::from_secs(3600);
            let hourly_total: u64 = window.entries.iter()
                .filter(|e| e.timestamp >= one_hour_ago)
                .map(|e| e.input_tokens + e.output_tokens)
                .sum();
            let hourly_requests: u64 = window.entries.iter()
                .filter(|e| e.timestamp >= one_hour_ago)
                .count() as u64;

            if hourly_total > window.hourly_limit && !window.alerted_this_hour {
                window.alerted_this_hour = true;
                let alert = TokenAlert {
                    hourly_tokens: hourly_total,
                    hourly_limit: window.hourly_limit,
                    hourly_requests,
                    total_tokens: total,
                    message: format!(
                        "⚠️ Token alert: {}M tokens in last hour ({} requests). Limit: {}M/hr. Total since start: {}M.",
                        hourly_total / 1_000_000,
                        hourly_requests,
                        window.hourly_limit / 1_000_000,
                        total / 1_000_000,
                    ),
                };
                tracing::warn!("{}", alert.message);

                if let Some(alert_fn) = self.alert_fn.get() {
                    alert_fn(alert);
                }
            }
        }
    }

    /// Get total input tokens.
    pub fn total_input(&self) -> u64 {
        self.total_input.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get total output tokens.
    pub fn total_output(&self) -> u64 {
        self.total_output.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get total requests.
    pub fn total_requests(&self) -> u64 {
        self.total_requests.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get total cache read tokens.
    pub fn total_cache_read(&self) -> u64 {
        self.total_cache_read.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get total cache write tokens.
    pub fn total_cache_write(&self) -> u64 {
        self.total_cache_write.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get snapshot of all stats (includes hourly window).
    pub fn snapshot(&self) -> TokenStats {
        use std::sync::atomic::Ordering;
        let (hourly_tokens, hourly_requests) = if let Ok(window) = self.window.lock() {
            let one_hour_ago = std::time::Instant::now() - std::time::Duration::from_secs(3600);
            let tokens: u64 = window.entries.iter()
                .filter(|e| e.timestamp >= one_hour_ago)
                .map(|e| e.input_tokens + e.output_tokens)
                .sum();
            let requests = window.entries.iter()
                .filter(|e| e.timestamp >= one_hour_ago)
                .count() as u64;
            (tokens, requests)
        } else {
            (0, 0)
        };

        TokenStats {
            total_input: self.total_input.load(Ordering::Relaxed),
            total_output: self.total_output.load(Ordering::Relaxed),
            total_requests: self.total_requests.load(Ordering::Relaxed),
            total_cache_read: self.total_cache_read.load(Ordering::Relaxed),
            total_cache_write: self.total_cache_write.load(Ordering::Relaxed),
            hourly_tokens,
            hourly_requests,
        }
    }
}

impl Default for TokenTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of token stats for serialization.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TokenStats {
    pub total_input: u64,
    pub total_output: u64,
    pub total_requests: u64,
    pub total_cache_read: u64,
    pub total_cache_write: u64,
    /// Tokens used in last hour (sliding window)
    pub hourly_tokens: u64,
    /// Requests in last hour
    pub hourly_requests: u64,
}

/// Global token tracker instance.
static TOKEN_TRACKER: std::sync::OnceLock<TokenTracker> = std::sync::OnceLock::new();

/// Get the global token tracker.
pub fn token_tracker() -> &'static TokenTracker {
    TOKEN_TRACKER.get_or_init(TokenTracker::new)
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
    /// Return the model name this client is configured to use.
    fn model_name(&self) -> &str;

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
/// OAuth header constants for Claude Max / Claude Code compatibility.
const OAUTH_BETA_HEADER: &str = "claude-code-20250219,oauth-2025-04-20,fine-grained-tool-streaming-2025-05-14,interleaved-thinking-2025-05-14";
const OAUTH_USER_AGENT: &str = "claude-cli/2.1.62";

/// Claude Code identity string — REQUIRED for OAuth tokens to access non-haiku models.
/// Without this in the system prompt, Anthropic's API restricts OAuth to haiku-only.
const CLAUDE_CODE_IDENTITY: &str = "You are Claude Code, Anthropic's official CLI for Claude.";

pub struct AnthropicClient {
    client: reqwest::Client,
    auth: AuthMode,
    /// Auth profile manager for multi-token rotation (optional).
    profile_manager: Option<Arc<Mutex<AuthProfileManager>>>,
    /// Current profile ID being used (if using profile rotation).
    current_profile_id: Arc<Mutex<Option<String>>>,
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

        // Initialize auth profile manager (for multi-token rotation)
        let profile_manager = match AuthProfileManager::new(config.auth_profiles_path.as_deref()) {
            Ok(mgr) => {
                if mgr.has_profiles("anthropic") {
                    let profile_count = mgr.store().list_profiles_for_provider("anthropic").len();
                    tracing::info!(
                        "Auth profile rotation enabled: {} profile(s) for anthropic",
                        profile_count
                    );
                    Some(Arc::new(Mutex::new(mgr)))
                } else {
                    tracing::debug!("No auth profiles found, using single-token mode");
                    None
                }
            }
            Err(e) => {
                tracing::warn!("Failed to load auth profiles, using single-token mode: {}", e);
                None
            }
        };

        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()?,
            auth,
            profile_manager,
            current_profile_id: Arc::new(Mutex::new(None)),
            model: config.model.clone(),
            max_tokens: config.max_tokens.unwrap_or_else(|| Self::default_max_tokens(&config.model)),
            base_url,
        })
    }

    /// Returns the model's maximum output tokens.
    /// Each provider knows its own models' limits.
    /// Model max output tokens per Anthropic docs:
    /// https://docs.anthropic.com/en/docs/about-claude/models
    fn default_max_tokens(model: &str) -> u32 {
        if model.contains("opus-4-6") {
            128000
        } else if model.contains("opus-4-5") || model.contains("opus") {
            64000
        } else if model.contains("sonnet-4-6") {
            64000
        } else if model.contains("sonnet-4-5") || model.contains("sonnet") {
            16000
        } else if model.contains("haiku-4-5") || model.contains("haiku") {
            64000
        } else {
            16000
        }
    }

    /// Apply auth headers to a request builder using the given auth mode.
    async fn apply_auth_mode(&self, mut req: reqwest::RequestBuilder, auth: &AuthMode) -> anyhow::Result<reqwest::RequestBuilder> {
        match auth {
            AuthMode::ApiKey(key) => {
                req = req.header("x-api-key", key);
            }
            AuthMode::OAuthToken(token) => {
                req = req
                    .header("Authorization", format!("Bearer {}", token))
                    .header("anthropic-beta", OAUTH_BETA_HEADER)
                    .header("user-agent", OAUTH_USER_AGENT)
                    .header("x-app", "cli");
            }
            AuthMode::OAuthManaged(manager) => {
                let token = manager.get_token().await?;
                req = req
                    .header("Authorization", format!("Bearer {}", token))
                    .header("anthropic-beta", OAUTH_BETA_HEADER)
                    .header("user-agent", OAUTH_USER_AGENT)
                    .header("x-app", "cli");
            }
        }
        Ok(req)
    }

    /// Apply auth headers to a request builder (uses primary auth).
    async fn apply_auth(&self, req: reqwest::RequestBuilder) -> anyhow::Result<reqwest::RequestBuilder> {
        self.apply_auth_mode(req, &self.auth).await
    }

    /// Apply auth from a profile credential.
    async fn apply_profile_auth(
        &self,
        mut req: reqwest::RequestBuilder,
        credential: &AuthProfileCredential,
    ) -> anyhow::Result<reqwest::RequestBuilder> {
        if credential.is_keychain() {
            // Use the primary auth (which should be OAuthManaged from Keychain)
            return self.apply_auth(req).await;
        }

        match credential {
            AuthProfileCredential::ApiKey { key, .. } => {
                req = req.header("x-api-key", key);
            }
            AuthProfileCredential::Token { token, .. } => {
                req = req
                    .header("Authorization", format!("Bearer {}", token))
                    .header("anthropic-beta", OAUTH_BETA_HEADER)
                    .header("user-agent", OAUTH_USER_AGENT)
                    .header("x-app", "cli");
            }
            AuthProfileCredential::OAuth { access, .. } => {
                req = req
                    .header("Authorization", format!("Bearer {}", access))
                    .header("anthropic-beta", OAUTH_BETA_HEADER)
                    .header("user-agent", OAUTH_USER_AGENT)
                    .header("x-app", "cli");
            }
        }
        Ok(req)
    }

    /// Get the next available profile for rotation.
    /// Returns (profile_id, credential) or None if no profiles available.
    async fn next_profile(&self) -> Option<(String, AuthProfileCredential)> {
        let manager = self.profile_manager.as_ref()?;
        let mut mgr = manager.lock().await;

        let profile_id = mgr.next_profile("anthropic")?;
        let credential = mgr.get_credential(&profile_id)?.clone();

        Some((profile_id, credential))
    }

    /// Mark a profile as used successfully.
    async fn mark_profile_used(&self, profile_id: &str) {
        if let Some(ref manager) = self.profile_manager {
            let mut mgr = manager.lock().await;
            mgr.mark_used(profile_id);
        }
    }

    /// Mark a profile as failed.
    async fn mark_profile_failure(&self, profile_id: &str, reason: AuthProfileFailureReason) {
        if let Some(ref manager) = self.profile_manager {
            let mut mgr = manager.lock().await;
            mgr.mark_failure(profile_id, reason);
        }
    }

    /// Map HTTP status code to failure reason.
    fn status_to_failure_reason(status: u16) -> AuthProfileFailureReason {
        match status {
            401 => AuthProfileFailureReason::Auth,
            403 => AuthProfileFailureReason::AuthPermanent,
            429 => AuthProfileFailureReason::RateLimit,
            500 | 502 | 503 => AuthProfileFailureReason::Overloaded,
            529 => AuthProfileFailureReason::Overloaded,
            _ => AuthProfileFailureReason::Unknown,
        }
    }

    /// Force-refresh the OAuth token (call after 401 errors).
    #[allow(dead_code)]
    async fn force_refresh_token(&self) -> anyhow::Result<()> {
        if let AuthMode::OAuthManaged(manager) = &self.auth {
            manager.refresh().await?;
        }
        Ok(())
    }

    /// Build the system prompt value for the API request.
    /// For OAuth tokens, injects the Claude Code identity prefix (required for non-haiku access).
    fn build_system_value(&self, system: &str) -> serde_json::Value {
        let is_oauth = matches!(&self.auth, AuthMode::OAuthToken(_) | AuthMode::OAuthManaged(_));
        if is_oauth {
            // OAuth tokens MUST include Claude Code identity to access sonnet/opus models.
            // Format as array of content blocks (matching the official SDK).
            serde_json::json!([
                {"type": "text", "text": CLAUDE_CODE_IDENTITY},
                {"type": "text", "text": system}
            ])
        } else {
            serde_json::json!(system)
        }
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
    fn model_name(&self) -> &str {
        &self.model
    }

    async fn chat(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> anyhow::Result<LlmResponse> {
        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "system": self.build_system_value(system),
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

        // Retry loop with exponential backoff and profile rotation
        let mut attempt = 0;
        let mut last_error: Option<anyhow::Error> = None;
        let mut current_profile: Option<(String, AuthProfileCredential)> = None;
        let mut tried_profiles: Vec<String> = Vec::new();

        // Get max attempts: base retries + profile count
        let profile_count = if let Some(ref mgr) = self.profile_manager {
            mgr.lock().await.store().list_profiles_for_provider("anthropic").len()
        } else {
            0
        };
        let total_retries = MAX_RETRIES + profile_count as u32 * 2;

        loop {
            attempt += 1;

            // Determine which auth to use
            let (auth_label, use_profile) = if let Some((ref id, _)) = current_profile {
                (format!("profile:{}", id), true)
            } else {
                let label = match &self.auth {
                    AuthMode::OAuthToken(_) => "oauth",
                    AuthMode::OAuthManaged(_) => "oauth-managed",
                    AuthMode::ApiKey(_) => "api_key",
                };
                (label.to_string(), false)
            };

            tracing::info!(
                "LLM request attempt {}/{} → model={} url={}/v1/messages auth={}",
                attempt, total_retries, self.model, self.base_url, auth_label
            );

            let req = self
                .client
                .post(format!("{}/v1/messages", self.base_url))
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json");

            // Apply auth headers — use profile if set, otherwise primary
            let req = if use_profile {
                if let Some((_, ref cred)) = current_profile {
                    self.apply_profile_auth(req, cred).await?
                } else {
                    self.apply_auth(req).await?
                }
            } else {
                self.apply_auth(req).await?
            };

            tracing::debug!("Anthropic request body: {}", serde_json::to_string_pretty(&body).unwrap_or_default());
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

            // Handle 401 — try refreshing OAuth token and retry once
            if status.as_u16() == 401 {
                // Mark current profile as failed if using profile rotation
                if let Some((ref id, _)) = current_profile {
                    self.mark_profile_failure(id, AuthProfileFailureReason::Auth).await;
                    tried_profiles.push(id.clone());
                }

                if let AuthMode::OAuthManaged(manager) = &self.auth {
                    if attempt <= 2 {
                        tracing::warn!("Got 401, attempting OAuth token refresh...");
                        match manager.refresh().await {
                            Ok(_) => {
                                tracing::info!("Token refreshed, retrying request");
                                continue;
                            }
                            Err(e) => {
                                tracing::error!("Token refresh failed: {}", e);
                            }
                        }
                    }
                }
            }

            // Check for client errors - don't retry these (except for profile rotation on 401)
            if is_client_error(status) && status.as_u16() != 401 {
                let resp_body: serde_json::Value = resp.json().await?;
                tracing::error!("Anthropic API error body: {}", serde_json::to_string_pretty(&resp_body).unwrap_or_default());
                let error_msg = resp_body["error"]["message"]
                    .as_str()
                    .unwrap_or("Unknown error");
                anyhow::bail!("Anthropic API error ({}): {}", status, error_msg);
            }

            // Check for retryable errors
            if should_retry(status) && attempt <= total_retries {
                // Mark current profile as failed
                if let Some((ref id, _)) = current_profile {
                    let reason = Self::status_to_failure_reason(status.as_u16());
                    self.mark_profile_failure(id, reason).await;
                    tried_profiles.push(id.clone());
                }

                // On 429/529/401, try rotating to next available profile
                if matches!(status.as_u16(), 401 | 429 | 529) {
                    // Try to get next profile (skipping already tried ones)
                    let next_profile = if let Some(ref manager) = self.profile_manager {
                        let mut mgr = manager.lock().await;
                        let order = mgr.store_mut().resolve_auth_order("anthropic");

                        // Find first profile not yet tried
                        let mut found = None;
                        for profile_id in order {
                            if !tried_profiles.contains(&profile_id) {
                                if let Some(cred) = mgr.get_credential(&profile_id).cloned() {
                                    found = Some((profile_id, cred));
                                    break;
                                }
                            }
                        }
                        found
                    } else {
                        None
                    };

                    if let Some((profile_id, cred)) = next_profile {
                        tracing::warn!(
                            "Overloaded ({}) on attempt {}. Rotating to profile '{}'...",
                            status, attempt, profile_id
                        );
                        current_profile = Some((profile_id, cred));
                        // Short delay before trying next profile
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        last_error = Some(anyhow::anyhow!("HTTP {}", status));
                        continue;
                    }
                }

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
                    status, attempt, total_retries, backoff
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

            // Success! Mark profile as used
            if let Some((ref id, _)) = current_profile {
                self.mark_profile_used(id).await;
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

            // Track token usage
            token_tracker().record(&usage);

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
            "system": self.build_system_value(system),
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

        // Retry loop with exponential backoff and profile rotation (same pattern as chat())
        let mut attempt = 0;
        let mut last_error: Option<anyhow::Error> = None;
        let mut current_profile: Option<(String, AuthProfileCredential)> = None;
        let mut tried_profiles: Vec<String> = Vec::new();

        // Get max attempts: base retries + profile count
        let profile_count = if let Some(ref mgr) = self.profile_manager {
            mgr.lock().await.store().list_profiles_for_provider("anthropic").len()
        } else {
            0
        };
        let total_retries = MAX_RETRIES + profile_count as u32 * 2;

        let resp = loop {
            attempt += 1;

            // Determine which auth to use
            let (auth_label, use_profile) = if let Some((ref id, _)) = current_profile {
                (format!("profile:{}", id), true)
            } else {
                // Try to get a profile for streaming
                if current_profile.is_none() && self.profile_manager.is_some() {
                    current_profile = self.next_profile().await;
                }
                if let Some((ref id, _)) = current_profile {
                    (format!("profile:{}", id), true)
                } else {
                    let label = match &self.auth {
                        AuthMode::OAuthToken(_) => "oauth",
                        AuthMode::OAuthManaged(_) => "oauth-managed",
                        AuthMode::ApiKey(_) => "api_key",
                    };
                    (label.to_string(), false)
                }
            };

            tracing::info!(
                "LLM stream request attempt {}/{} → model={} url={}/v1/messages auth={}",
                attempt, total_retries, self.model, self.base_url, auth_label
            );

            let req = self
                .client
                .post(format!("{}/v1/messages", self.base_url))
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json");

            // Apply auth headers — use profile if set, otherwise primary
            let req = if use_profile {
                if let Some((_, ref cred)) = current_profile {
                    self.apply_profile_auth(req, cred).await?
                } else {
                    self.apply_auth(req).await?
                }
            } else {
                self.apply_auth(req).await?
            };

            let resp_result = req.json(&body).send().await;
            let resp = match resp_result {
                Ok(r) => r,
                Err(e) => {
                    if attempt <= MAX_RETRIES {
                        let backoff = INITIAL_BACKOFF_MS * 2u64.pow(attempt - 1);
                        tracing::warn!(
                            "Stream request failed (attempt {}/{}): {}. Retrying in {}ms...",
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

            // Handle 401 — try refreshing OAuth token and retry
            if status.as_u16() == 401 {
                // Mark current profile as failed if using profile rotation
                if let Some((ref id, _)) = current_profile {
                    self.mark_profile_failure(id, AuthProfileFailureReason::Auth).await;
                    tried_profiles.push(id.clone());
                }

                if let AuthMode::OAuthManaged(manager) = &self.auth {
                    if attempt <= 2 {
                        tracing::warn!("Got 401 on stream, attempting OAuth token refresh...");
                        match manager.refresh().await {
                            Ok(_) => {
                                tracing::info!("Token refreshed, retrying stream request");
                                current_profile = None; // Reset to try primary auth again
                                continue;
                            }
                            Err(e) => {
                                tracing::error!("Token refresh failed: {}", e);
                            }
                        }
                    }
                }
            }

            // Check for client errors - don't retry these (except for profile rotation on 401)
            if is_client_error(status) && status.as_u16() != 401 {
                let resp_body: serde_json::Value = resp.json().await?;
                tracing::error!("Anthropic stream API error body: {}", serde_json::to_string_pretty(&resp_body).unwrap_or_default());
                let error_msg = resp_body["error"]["message"]
                    .as_str()
                    .unwrap_or("Unknown error");
                anyhow::bail!("Anthropic API error ({}): {}", status, error_msg);
            }

            // Check for retryable errors
            if should_retry(status) && attempt <= total_retries {
                // Mark current profile as failed
                if let Some((ref id, _)) = current_profile {
                    let reason = Self::status_to_failure_reason(status.as_u16());
                    self.mark_profile_failure(id, reason).await;
                    tried_profiles.push(id.clone());
                }

                // On 429/529/401, try rotating to next available profile
                if matches!(status.as_u16(), 401 | 429 | 529) {
                    // Try to get next profile (skipping already tried ones)
                    let next_profile = if let Some(ref manager) = self.profile_manager {
                        let mut mgr = manager.lock().await;
                        let order = mgr.store_mut().resolve_auth_order("anthropic");

                        // Find first profile not yet tried
                        let mut found = None;
                        for profile_id in order {
                            if !tried_profiles.contains(&profile_id) {
                                if let Some(cred) = mgr.get_credential(&profile_id).cloned() {
                                    found = Some((profile_id, cred));
                                    break;
                                }
                            }
                        }
                        found
                    } else {
                        None
                    };

                    if let Some((profile_id, cred)) = next_profile {
                        tracing::warn!(
                            "Stream overloaded ({}) on attempt {}. Rotating to profile '{}'...",
                            status, attempt, profile_id
                        );
                        current_profile = Some((profile_id, cred));
                        // Short delay before trying next profile
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        last_error = Some(anyhow::anyhow!("HTTP {}", status));
                        continue;
                    }
                }

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
                    "Stream retryable error {} (attempt {}/{}). Retrying in {}ms...",
                    status, attempt, total_retries, backoff
                );

                tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
                last_error = Some(anyhow::anyhow!("HTTP {}", status));
                continue;
            }

            // Non-retryable error
            if !status.is_success() {
                let resp_body: serde_json::Value = resp.json().await?;
                let error_msg = resp_body["error"]["message"]
                    .as_str()
                    .unwrap_or("Unknown error");

                // If we've exhausted retries, include last error info
                if let Some(le) = &last_error {
                    anyhow::bail!(
                        "Anthropic stream API error ({}) after {} attempts: {} (last error: {})",
                        status, attempt, error_msg, le
                    );
                }
                anyhow::bail!("Anthropic stream API error ({}): {}", status, error_msg);
            }

            // Success! Mark profile as used
            if let Some((ref id, _)) = current_profile {
                self.mark_profile_used(id).await;
            }

            break resp;
        }; // end retry loop

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

            // Track token usage for streaming
            token_tracker().record(&usage);

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
            max_tokens: config.max_tokens.unwrap_or_else(|| Self::default_max_tokens(&config.model)),
            base_url,
        })
    }

    fn default_max_tokens(model: &str) -> u32 {
        if model.contains("gpt-4o") {
            16384
        } else if model.contains("gpt-4") {
            8192
        } else if model.contains("o1") || model.contains("o3") {
            16384
        } else {
            8192
        }
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
    fn model_name(&self) -> &str {
        &self.model
    }

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

        // Track token usage
        token_tracker().record(&usage);

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
        // Token tracking is done in chat() call
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
            max_tokens: config.max_tokens.unwrap_or_else(|| Self::default_max_tokens(&config.model)),
        })
    }

    fn default_max_tokens(model: &str) -> u32 {
        if model.contains("pro") {
            8192
        } else if model.contains("flash") {
            8192
        } else {
            8192
        }
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
    fn model_name(&self) -> &str {
        &self.model
    }

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

        // Track token usage
        token_tracker().record(&usage);

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
        // Token tracking is done in chat() call
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
