//! Telegram channel adapter using raw HTTP API.
//!
//! Uses long polling (getUpdates) — simple, no webhook needed.

use std::sync::Arc;

use crate::agent::AgentRunner;
use crate::config::TelegramConfig;

const TELEGRAM_API: &str = "https://api.telegram.org";

/// Telegram bot client.
struct TelegramBot {
    client: reqwest::Client,
    token: String,
    config: TelegramConfig,
    runner: Arc<AgentRunner>,
}

impl TelegramBot {
    fn new(config: TelegramConfig, runner: Arc<AgentRunner>) -> Self {
        Self {
            client: reqwest::Client::new(),
            token: config.bot_token.clone(),
            config,
            runner,
        }
    }

    fn api_url(&self, method: &str) -> String {
        format!("{}/bot{}/{}", TELEGRAM_API, self.token, method)
    }

    /// Send a text message.
    async fn send_message(&self, chat_id: i64, text: &str) -> anyhow::Result<()> {
        // Split long messages (Telegram limit: 4096 chars)
        let chunks = split_message(text, 4096);
        for chunk in chunks {
            self.client
                .post(self.api_url("sendMessage"))
                .json(&serde_json::json!({
                    "chat_id": chat_id,
                    "text": chunk,
                    "parse_mode": "Markdown",
                }))
                .send()
                .await?;
        }
        Ok(())
    }

    /// Send a voice message.
    async fn send_voice(&self, chat_id: i64, ogg_path: &str) -> anyhow::Result<()> {
        let file_bytes = tokio::fs::read(ogg_path).await?;
        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name("voice.ogg")
            .mime_str("audio/ogg")?;

        let form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part("voice", part);

        self.client
            .post(self.api_url("sendVoice"))
            .multipart(form)
            .send()
            .await?;

        Ok(())
    }

    /// Process a single update.
    async fn handle_update(&self, update: &serde_json::Value) -> anyhow::Result<()> {
        let message = match update.get("message") {
            Some(m) => m,
            None => return Ok(()), // Skip non-message updates
        };

        let chat_id = message["chat"]["id"].as_i64().unwrap_or(0);
        let user_id = message["from"]["id"].as_i64().unwrap_or(0);
        let text = message["text"].as_str().unwrap_or("");

        if text.is_empty() {
            // TODO: Handle voice messages (STT)
            return Ok(());
        }

        // Check access
        if !self.config.allowed_users.is_empty()
            && !self.config.allowed_users.contains(&user_id)
        {
            tracing::warn!("Unauthorized user: {}", user_id);
            return Ok(());
        }

        // Build session key
        let session_key = format!("telegram:{}", chat_id);
        let user_id_str = user_id.to_string();

        tracing::info!("Message from user {} in chat {}: {}", user_id, chat_id, 
            if text.len() > 50 { &text[..50] } else { text });

        // Process with agent
        match self
            .runner
            .process_message(&session_key, text, Some(&user_id_str), Some("telegram"))
            .await
        {
            Ok(response) => {
                if !response.is_empty() && response != "NO_REPLY" {
                    self.send_message(chat_id, &response).await?;
                }
            }
            Err(e) => {
                tracing::error!("Agent error: {}", e);
                self.send_message(chat_id, &format!("⚠️ Error: {}", e)).await?;
            }
        }

        Ok(())
    }

    /// Run the long-polling loop.
    async fn run(&self) -> anyhow::Result<()> {
        let mut offset: i64 = 0;
        tracing::info!("Telegram bot started. Polling for updates...");

        loop {
            let resp = self
                .client
                .post(self.api_url("getUpdates"))
                .json(&serde_json::json!({
                    "offset": offset,
                    "timeout": 30,
                    "allowed_updates": ["message"],
                }))
                .send()
                .await;

            match resp {
                Ok(r) => {
                    let body: serde_json::Value = r.json().await?;
                    if let Some(updates) = body["result"].as_array() {
                        for update in updates {
                            if let Some(id) = update["update_id"].as_i64() {
                                offset = id + 1;
                            }
                            if let Err(e) = self.handle_update(update).await {
                                tracing::error!("Update handling error: {}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Polling error: {}. Retrying in 5s...", e);
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    }
}

/// Split a message into chunks respecting Telegram's character limit.
fn split_message(text: &str, max_len: usize) -> Vec<&str> {
    if text.len() <= max_len {
        return vec![text];
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let end = std::cmp::min(start + max_len, text.len());
        // Try to split at a newline
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

/// Start the Telegram channel.
pub async fn start(config: TelegramConfig, runner: Arc<AgentRunner>) -> anyhow::Result<()> {
    let bot = TelegramBot::new(config, runner);
    bot.run().await
}
