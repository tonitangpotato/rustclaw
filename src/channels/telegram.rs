//! Telegram channel adapter using raw HTTP API.
//!
//! Uses long polling (getUpdates) — simple, no webhook needed.

use std::sync::Arc;

use crate::agent::AgentRunner;
use crate::config::TelegramConfig;
use crate::stt;
use crate::tts::{synthesize, TtsConfig};

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

    /// Send a text message with MarkdownV2 formatting, falling back to plain text.
    async fn send_message(&self, chat_id: i64, text: &str) -> anyhow::Result<()> {
        // Split long messages (Telegram limit: 4096 chars)
        let chunks = split_message(text, 4096);
        for chunk in chunks {
            // Try MarkdownV2 first
            let escaped = escape_markdown_v2(chunk);
            let response = self.client
                .post(self.api_url("sendMessage"))
                .json(&serde_json::json!({
                    "chat_id": chat_id,
                    "text": escaped,
                    "parse_mode": "MarkdownV2",
                }))
                .send()
                .await?;

            // If MarkdownV2 fails (400 error), retry without parse_mode
            if response.status().as_u16() == 400 {
                tracing::debug!("MarkdownV2 failed, retrying without parse_mode");
                self.client
                    .post(self.api_url("sendMessage"))
                    .json(&serde_json::json!({
                        "chat_id": chat_id,
                        "text": chunk,
                    }))
                    .send()
                    .await?;
            }
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

        // Handle voice messages
        if let Some(voice) = message.get("voice") {
            return self.handle_voice_message(chat_id, user_id, voice).await;
        }

        if text.is_empty() {
            // Check for audio (not voice note)
            if message.get("audio").is_some() {
                self.send_message(chat_id, "🎵 Audio files not yet supported. Please send a voice message.").await?;
                return Ok(());
            }
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

        // Send "typing" indicator
        let _ = self
            .client
            .post(self.api_url("sendChatAction"))
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "action": "typing",
            }))
            .send()
            .await;

        // Process with agent
        match self
            .runner
            .process_message(&session_key, text, Some(&user_id_str), Some("telegram"))
            .await
        {
            Ok(response) => {
                let trimmed = response.trim();
                if !trimmed.is_empty()
                    && trimmed != "NO_REPLY"
                    && trimmed != "HEARTBEAT_OK"
                {
                    // Check if response should be sent as voice
                    if let Some(voice_text) = Self::extract_voice_text(trimmed) {
                        // Send "recording voice" indicator
                        let _ = self
                            .client
                            .post(self.api_url("sendChatAction"))
                            .json(&serde_json::json!({
                                "chat_id": chat_id,
                                "action": "record_voice",
                            }))
                            .send()
                            .await;

                        // Synthesize TTS
                        let tts_config = TtsConfig::default();
                        match synthesize(&voice_text, &tts_config).await {
                            Ok(ogg_path) => {
                                if let Err(e) = self.send_voice(chat_id, &ogg_path).await {
                                    tracing::error!("Failed to send voice: {}", e);
                                    // Fallback to text
                                    self.send_message(chat_id, trimmed).await?;
                                }
                            }
                            Err(e) => {
                                tracing::error!("TTS synthesis failed: {}", e);
                                // Fallback to text
                                self.send_message(chat_id, trimmed).await?;
                            }
                        }
                    } else {
                        self.send_message(chat_id, trimmed).await?;
                    }
                }
            }
            Err(e) => {
                tracing::error!("Agent error: {}", e);
                self.send_message(chat_id, &format!("⚠️ Error: {}", e)).await?;
            }
        }

        Ok(())
    }

    /// Extract voice text if response should be sent as voice.
    /// Returns None if not a voice response, Some(text) with prefix stripped otherwise.
    fn extract_voice_text(response: &str) -> Option<String> {
        // Check for VOICE: prefix
        if let Some(rest) = response.strip_prefix("VOICE:") {
            return Some(rest.trim().to_string());
        }
        // Check for 🔊 prefix
        if let Some(rest) = response.strip_prefix("🔊") {
            return Some(rest.trim().to_string());
        }
        None
    }

    /// Handle a voice message by downloading, transcribing, and processing.
    async fn handle_voice_message(
        &self,
        chat_id: i64,
        user_id: i64,
        voice: &serde_json::Value,
    ) -> anyhow::Result<()> {
        // Check access first
        if !self.config.allowed_users.is_empty()
            && !self.config.allowed_users.contains(&user_id)
        {
            tracing::warn!("Unauthorized user for voice: {}", user_id);
            return Ok(());
        }

        let file_id = match voice["file_id"].as_str() {
            Some(id) => id,
            None => {
                self.send_message(chat_id, "⚠️ Could not get voice file ID").await?;
                return Ok(());
            }
        };

        tracing::info!("Voice message from user {} in chat {}", user_id, chat_id);

        // Send "typing" indicator
        let _ = self
            .client
            .post(self.api_url("sendChatAction"))
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "action": "typing",
            }))
            .send()
            .await;

        // Step 1: Get file path via getFile API
        let file_info = self
            .client
            .post(self.api_url("getFile"))
            .json(&serde_json::json!({ "file_id": file_id }))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        let file_path = match file_info["result"]["file_path"].as_str() {
            Some(path) => path,
            None => {
                self.send_message(chat_id, "⚠️ Could not get file path from Telegram").await?;
                return Ok(());
            }
        };

        // Step 2: Download the file
        let download_url = format!(
            "{}/file/bot{}/{}",
            TELEGRAM_API, self.token, file_path
        );
        
        let file_bytes = self
            .client
            .get(&download_url)
            .send()
            .await?
            .bytes()
            .await?;

        // Save to temp file
        let ogg_path = "/tmp/rustclaw_voice_in.ogg";
        tokio::fs::write(ogg_path, &file_bytes).await?;
        tracing::debug!("Downloaded voice to {}", ogg_path);

        // Step 3: Transcribe using STT
        let transcription = stt::transcribe(ogg_path).await?;
        tracing::info!("Transcribed: {}", &transcription[..transcription.len().min(50)]);

        // Clean up the input file
        let _ = tokio::fs::remove_file(ogg_path).await;

        // Step 4: Process through agent with [Voice message] prefix
        let user_message = format!("[Voice message] {}", transcription);
        let session_key = format!("telegram:{}", chat_id);
        let user_id_str = user_id.to_string();

        match self
            .runner
            .process_message(&session_key, &user_message, Some(&user_id_str), Some("telegram"))
            .await
        {
            Ok(response) => {
                let trimmed = response.trim();
                if !trimmed.is_empty()
                    && trimmed != "NO_REPLY"
                    && trimmed != "HEARTBEAT_OK"
                {
                    // Check if response should be sent as voice
                    if let Some(voice_text) = Self::extract_voice_text(trimmed) {
                        // Send "recording voice" indicator
                        let _ = self
                            .client
                            .post(self.api_url("sendChatAction"))
                            .json(&serde_json::json!({
                                "chat_id": chat_id,
                                "action": "record_voice",
                            }))
                            .send()
                            .await;

                        // Synthesize TTS
                        let tts_config = TtsConfig::default();
                        match synthesize(&voice_text, &tts_config).await {
                            Ok(ogg_path) => {
                                if let Err(e) = self.send_voice(chat_id, &ogg_path).await {
                                    tracing::error!("Failed to send voice: {}", e);
                                    self.send_message(chat_id, trimmed).await?;
                                }
                            }
                            Err(e) => {
                                tracing::error!("TTS synthesis failed: {}", e);
                                self.send_message(chat_id, trimmed).await?;
                            }
                        }
                    } else {
                        self.send_message(chat_id, trimmed).await?;
                    }
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

/// Escape special characters for Telegram MarkdownV2.
/// Characters that need escaping: _ * [ ] ( ) ~ ` > # + - = | { } . !
fn escape_markdown_v2(text: &str) -> String {
    let special_chars = ['_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!'];
    let mut result = String::with_capacity(text.len() * 2);
    
    for c in text.chars() {
        if special_chars.contains(&c) {
            result.push('\\');
        }
        result.push(c);
    }
    
    result
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
