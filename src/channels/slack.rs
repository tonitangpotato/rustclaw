//! Slack channel adapter using Socket Mode.
//!
//! Connects via WebSocket for real-time events. Uses Socket Mode which requires
//! an app-level token in addition to the bot token.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

use crate::agent::AgentRunner;
use crate::config::SlackConfig;

const SLACK_API: &str = "https://slack.com/api";

/// Slack bot client.
struct SlackBot {
    client: reqwest::Client,
    config: SlackConfig,
    runner: Arc<AgentRunner>,
    /// Bot user ID (fetched on startup)
    bot_user_id: String,
}

impl SlackBot {
    async fn new(config: SlackConfig, runner: Arc<AgentRunner>) -> anyhow::Result<Self> {
        let client = reqwest::Client::new();

        // Get bot user ID via auth.test
        let resp: AuthTestResponse = client
            .post(format!("{}/auth.test", SLACK_API))
            .bearer_auth(&config.bot_token)
            .send()
            .await?
            .json()
            .await?;

        if !resp.ok {
            anyhow::bail!("Slack auth.test failed: {:?}", resp.error);
        }

        let bot_user_id = resp.user_id.ok_or_else(|| anyhow::anyhow!("No user_id in auth.test"))?;
        tracing::info!("Slack bot authenticated as user_id={}", bot_user_id);

        Ok(Self {
            client,
            config,
            runner,
            bot_user_id,
        })
    }

    /// Get WebSocket URL for Socket Mode.
    async fn get_socket_url(&self) -> anyhow::Result<String> {
        let resp: AppsConnectionsOpenResponse = self
            .client
            .post(format!("{}/apps.connections.open", SLACK_API))
            .bearer_auth(&self.config.app_token)
            .send()
            .await?
            .json()
            .await?;

        if !resp.ok {
            anyhow::bail!("apps.connections.open failed: {:?}", resp.error);
        }

        resp.url.ok_or_else(|| anyhow::anyhow!("No URL in response"))
    }

    /// Check if message mentions the bot.
    fn is_mentioned(&self, text: &str) -> bool {
        // Slack mentions are formatted as <@USER_ID>
        let mention = format!("<@{}>", self.bot_user_id);
        text.contains(&mention)
    }

    /// Strip bot mention from message text.
    fn strip_mention(&self, text: &str) -> String {
        let mention = format!("<@{}>", self.bot_user_id);
        text.replace(&mention, "").trim().to_string()
    }

    /// Check if we should process a message from this channel.
    fn should_process_channel(&self, channel: &str) -> bool {
        self.config.allowed_channels.is_empty()
            || self.config.allowed_channels.contains(&channel.to_string())
    }

    /// Convert text to Slack mrkdwn format.
    fn to_mrkdwn(text: &str) -> String {
        let mut result = text.to_string();

        // Unescape Telegram-style escapes
        let unescape_chars = ['_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!'];
        for c in unescape_chars {
            result = result.replace(&format!("\\{}", c), &c.to_string());
        }

        // Convert markdown links [text](url) to Slack format <url|text>
        let link_re = regex::Regex::new(r"\[([^\]]+)\]\(([^)]+)\)").unwrap();
        result = link_re.replace_all(&result, "<$2|$1>").to_string();

        // Slack uses *bold* (single asterisk) and _italic_ (single underscore)
        // Standard markdown uses **bold** and *italic*
        // This is a simple heuristic - convert **text** to *text*
        let bold_re = regex::Regex::new(r"\*\*([^*]+)\*\*").unwrap();
        result = bold_re.replace_all(&result, "*$1*").to_string();

        result
    }

    /// Send a message to a channel.
    async fn send_message(&self, channel: &str, text: &str, thread_ts: Option<&str>) -> anyhow::Result<()> {
        let mrkdwn = Self::to_mrkdwn(text);

        // Split long messages (Slack limit: 4000 chars for text field)
        let chunks = split_message(&mrkdwn, 4000);

        for chunk in chunks {
            let mut payload = serde_json::json!({
                "channel": channel,
                "text": chunk,
            });

            if let Some(ts) = thread_ts {
                payload["thread_ts"] = serde_json::json!(ts);
            }

            let resp: SlackResponse = self
                .client
                .post(format!("{}/chat.postMessage", SLACK_API))
                .bearer_auth(&self.config.bot_token)
                .json(&payload)
                .send()
                .await?
                .json()
                .await?;

            if !resp.ok {
                tracing::error!("Failed to send message: {:?}", resp.error);
            }
        }

        Ok(())
    }

    /// Upload a file to a channel.
    async fn upload_file(&self, channel: &str, file_path: &str, thread_ts: Option<&str>) -> anyhow::Result<()> {
        let file_bytes = tokio::fs::read(file_path).await?;
        let file_name = std::path::Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(file_name.to_string());

        let mut form = reqwest::multipart::Form::new()
            .text("channels", channel.to_string())
            .part("file", part);

        if let Some(ts) = thread_ts {
            form = form.text("thread_ts", ts.to_string());
        }

        let resp: SlackResponse = self
            .client
            .post(format!("{}/files.upload", SLACK_API))
            .bearer_auth(&self.config.bot_token)
            .multipart(form)
            .send()
            .await?
            .json()
            .await?;

        if !resp.ok {
            tracing::error!("Failed to upload file: {:?}", resp.error);
        }

        Ok(())
    }

    /// Send a response, handling file attachments.
    async fn send_response(&self, channel: &str, response: &str, thread_ts: Option<&str>) -> anyhow::Result<()> {
        // Check for FILE: patterns
        let file_re = regex::Regex::new(r"FILE:(/[^\s]+)").unwrap();
        let mut text_without_files = response.to_string();
        let mut files_to_send: Vec<String> = Vec::new();

        for cap in file_re.captures_iter(response) {
            let file_path = cap[1].to_string();
            files_to_send.push(file_path.clone());
            text_without_files = text_without_files.replace(&format!("FILE:{}", file_path), "");
        }

        let clean_text = text_without_files.trim();

        // Send text message
        if !clean_text.is_empty() {
            self.send_message(channel, clean_text, thread_ts).await?;
        }

        // Upload files
        for file_path in files_to_send {
            if std::path::Path::new(&file_path).exists() {
                if let Err(e) = self.upload_file(channel, &file_path, thread_ts).await {
                    tracing::error!("Failed to upload file {}: {}", file_path, e);
                    self.send_message(channel, &format!("⚠️ Failed to upload: {}", file_path), thread_ts).await?;
                }
            } else {
                self.send_message(channel, &format!("⚠️ File not found: {}", file_path), thread_ts).await?;
            }
        }

        Ok(())
    }

    /// Process a Socket Mode event.
    async fn handle_event(&self, envelope: &SocketModeEnvelope) -> anyhow::Result<()> {
        match envelope.event_type.as_deref() {
            Some("events_api") => {
                if let Some(payload) = &envelope.payload {
                    self.handle_events_api(payload).await?;
                }
            }
            Some(t) => {
                tracing::debug!("Ignoring Socket Mode event type: {}", t);
            }
            None => {}
        }
        Ok(())
    }

    /// Handle Events API payload.
    async fn handle_events_api(&self, payload: &serde_json::Value) -> anyhow::Result<()> {
        let event = match payload.get("event") {
            Some(e) => e,
            None => return Ok(()),
        };

        let event_type = event["type"].as_str().unwrap_or("");
        
        match event_type {
            "message" | "app_mention" => {
                self.handle_message(event).await?;
            }
            _ => {
                tracing::debug!("Ignoring event type: {}", event_type);
            }
        }

        Ok(())
    }

    /// Handle a message or app_mention event.
    async fn handle_message(&self, event: &serde_json::Value) -> anyhow::Result<()> {
        // Ignore bot messages and message_changed events
        if event.get("bot_id").is_some() || event.get("subtype").is_some() {
            return Ok(());
        }

        let channel = event["channel"].as_str().unwrap_or("");
        let user = event["user"].as_str().unwrap_or("");
        let text = event["text"].as_str().unwrap_or("");
        let thread_ts = event["thread_ts"].as_str().or_else(|| event["ts"].as_str());

        // Check channel filter
        if !self.should_process_channel(channel) {
            tracing::debug!("Ignoring message from channel {}", channel);
            return Ok(());
        }

        // Check if mentioned (for normal messages, not app_mention events)
        let event_type = event["type"].as_str().unwrap_or("");
        let is_app_mention = event_type == "app_mention";
        
        if !is_app_mention && !self.is_mentioned(text) {
            tracing::debug!("Ignoring message without mention");
            return Ok(());
        }

        // Strip mention
        let content = self.strip_mention(text);
        if content.is_empty() {
            self.send_message(channel, "Yes?", thread_ts).await?;
            return Ok(());
        }

        // Build session key
        let session_key = format!("slack:{}", channel);
        let user_id = user.to_string();

        tracing::info!(
            "Slack message from {} in {}: {}",
            user,
            channel,
            if content.len() > 50 { &content[..50] } else { &content }
        );

        // Process with agent
        match self
            .runner
            .process_message(&session_key, &content, Some(&user_id), Some("slack"))
            .await
        {
            Ok(response) => {
                let trimmed = response.trim();
                if !trimmed.is_empty() && trimmed != "NO_REPLY" && trimmed != "HEARTBEAT_OK" {
                    self.send_response(channel, trimmed, thread_ts).await?;
                }
            }
            Err(e) => {
                tracing::error!("Agent error: {}", e);
                self.send_message(channel, &format!("⚠️ Error: {}", e), thread_ts).await?;
            }
        }

        Ok(())
    }

    /// Run the Socket Mode connection loop.
    async fn run(&self) -> anyhow::Result<()> {
        loop {
            tracing::info!("Connecting to Slack Socket Mode...");
            
            let url = self.get_socket_url().await?;
            tracing::debug!("WebSocket URL: {}", url);

            match connect_async(&url).await {
                Ok((ws_stream, _)) => {
                    tracing::info!("Connected to Slack Socket Mode");
                    let (mut write, mut read) = ws_stream.split();

                    while let Some(msg_result) = read.next().await {
                        match msg_result {
                            Ok(WsMessage::Text(text)) => {
                                // Parse envelope
                                if let Ok(envelope) = serde_json::from_str::<SocketModeEnvelope>(&text) {
                                    // Acknowledge the event
                                    if let Some(envelope_id) = &envelope.envelope_id {
                                        let ack = serde_json::json!({
                                            "envelope_id": envelope_id
                                        });
                                        if let Err(e) = write.send(WsMessage::Text(ack.to_string().into())).await {
                                            tracing::error!("Failed to send ack: {}", e);
                                            break;
                                        }
                                    }

                                    // Handle the event
                                    if let Err(e) = self.handle_event(&envelope).await {
                                        tracing::error!("Event handling error: {}", e);
                                    }
                                }
                            }
                            Ok(WsMessage::Ping(data)) => {
                                if let Err(e) = write.send(WsMessage::Pong(data)).await {
                                    tracing::error!("Failed to send pong: {}", e);
                                    break;
                                }
                            }
                            Ok(WsMessage::Close(_)) => {
                                tracing::info!("WebSocket closed by server");
                                break;
                            }
                            Err(e) => {
                                tracing::error!("WebSocket error: {}", e);
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("WebSocket connection failed: {}", e);
                }
            }

            // Reconnect delay
            tracing::info!("Reconnecting in 5 seconds...");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }
}

/// Split a message into chunks respecting Slack's character limit.
fn split_message(text: &str, max_len: usize) -> Vec<&str> {
    if text.len() <= max_len {
        return vec![text];
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let end = std::cmp::min(start + max_len, text.len());
        let split_at = if end < text.len() {
            text[start..end]
                .rfind('\n')
                .map(|pos| start + pos + 1)
                .unwrap_or(end)
        } else {
            end
        };
        chunks.push(&text[start..split_at]);
        start = split_at;
    }

    chunks
}

// --- Slack API response types ---

#[derive(Debug, Deserialize)]
struct AuthTestResponse {
    ok: bool,
    user_id: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AppsConnectionsOpenResponse {
    ok: bool,
    url: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SlackResponse {
    ok: bool,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SocketModeEnvelope {
    envelope_id: Option<String>,
    #[serde(rename = "type")]
    event_type: Option<String>,
    payload: Option<serde_json::Value>,
}

/// Start the Slack channel.
pub async fn start(config: SlackConfig, runner: Arc<AgentRunner>) -> anyhow::Result<()> {
    let bot = SlackBot::new(config, runner).await?;
    bot.run().await
}
