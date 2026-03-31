//! Telegram channel adapter using raw HTTP API.
//!
//! Uses long polling (getUpdates) — simple, no webhook needed.

use std::sync::Arc;

use crate::agent::AgentRunner;
use crate::config::TelegramConfig;
use crate::stt;
use crate::text_utils;


const TELEGRAM_API: &str = "https://api.telegram.org";

/// Telegram bot client.
struct TelegramBot {
    client: reqwest::Client,
    token: String,
    config: TelegramConfig,
    runner: Arc<AgentRunner>,
    /// Bot username (fetched via getMe on startup)
    bot_username: String,
}

impl TelegramBot {
    async fn new(config: TelegramConfig, runner: Arc<AgentRunner>) -> anyhow::Result<Self> {
        let client = reqwest::Client::new();
        let token = config.bot_token.clone();
        
        // Fetch bot username via getMe
        let bot_username = Self::fetch_bot_username(&client, &token).await?;
        tracing::info!("Bot username: @{}", bot_username);
        
        Ok(Self {
            client,
            token,
            config,
            runner,
            bot_username,
        })
    }
    
    /// Return Telegram channel capabilities.
    fn capabilities(&self) -> crate::context::ChannelCapabilities {
        crate::context::ChannelCapabilities {
            name: "telegram".into(),
            supports_reactions: true,
            supports_inline_buttons: true,
            supports_voice: true,
            supports_reply_to: true,
            supports_typing: true,
            supports_markdown: true,
            supports_tables: false,
            max_message_length: 4096,
            format_notes: vec![
                "Use bullet lists instead of markdown tables — Telegram does not render them".into(),
                "Code blocks use triple backticks".into(),
                "For long responses, split into multiple messages".into(),
            ],
        }
    }

    /// Fetch bot username by calling getMe API.
    async fn fetch_bot_username(client: &reqwest::Client, token: &str) -> anyhow::Result<String> {
        let url = format!("{}/bot{}/getMe", TELEGRAM_API, token);
        let resp: serde_json::Value = client.get(&url).send().await?.json().await?;
        
        let username = resp["result"]["username"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Failed to get bot username from getMe"))?
            .to_string();
        
        Ok(username)
    }

    fn api_url(&self, method: &str) -> String {
        format!("{}/bot{}/{}", TELEGRAM_API, self.token, method)
    }

    /// Send a text message with MarkdownV2 formatting, falling back to plain text.
    /// If `reply_to` is provided, the message will be a reply to that message.
    /// Parses `[[button:text|callback_data]]` patterns into inline keyboard buttons.
    async fn send_message(&self, chat_id: i64, text: &str, reply_to: Option<i64>) -> anyhow::Result<()> {
        // Extract inline buttons from text
        let (clean_text, buttons) = extract_inline_buttons(text);
        
        // Split long messages (Telegram limit: 4096 chars)
        let chunks = text_utils::split_message(&clean_text, 4096);
        let total_chunks = chunks.len();
        
        for (i, chunk) in chunks.iter().enumerate() {
            // Only reply to the first chunk
            let reply_id = if i == 0 { reply_to } else { None };
            // Only add buttons to the last chunk
            let add_buttons = i == total_chunks - 1 && !buttons.is_empty();
            
            // Try MarkdownV2 first
            let escaped = escape_markdown_v2(chunk);
            let mut payload = serde_json::json!({
                "chat_id": chat_id,
                "text": escaped,
                "parse_mode": "MarkdownV2",
            });
            if let Some(msg_id) = reply_id {
                payload["reply_to_message_id"] = serde_json::json!(msg_id);
            }
            if add_buttons {
                payload["reply_markup"] = build_inline_keyboard(&buttons);
            }
            
            let response = self.client
                .post(self.api_url("sendMessage"))
                .json(&payload)
                .send()
                .await?;

            // If MarkdownV2 fails (400 error), retry without parse_mode
            if response.status().as_u16() == 400 {
                tracing::debug!("MarkdownV2 failed, retrying without parse_mode");
                let mut fallback = serde_json::json!({
                    "chat_id": chat_id,
                    "text": chunk,
                });
                if let Some(msg_id) = reply_id {
                    fallback["reply_to_message_id"] = serde_json::json!(msg_id);
                }
                if add_buttons {
                    fallback["reply_markup"] = build_inline_keyboard(&buttons);
                }
                self.client
                    .post(self.api_url("sendMessage"))
                    .json(&fallback)
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

    /// Synthesize TTS and send as voice message, with text fallback on failure.
    async fn send_as_voice(&self, chat_id: i64, text: &str, reply_to: Option<i64>) -> anyhow::Result<()> {
        let _ = self.client
            .post(self.api_url("sendChatAction"))
            .json(&serde_json::json!({ "chat_id": chat_id, "action": "record_voice" }))
            .send()
            .await;

        // Strip markdown formatting so TTS doesn't read *, #, |, ` etc.
        let clean_text = strip_markdown(text);
        let tts_config = crate::tts::TtsConfig::default();
        match crate::tts::synthesize(&clean_text, &tts_config).await {
            Ok(ogg_path) => {
                if let Err(e) = self.send_voice(chat_id, &ogg_path).await {
                    tracing::error!("Voice send failed: {}", e);
                    self.send_message(chat_id, text, reply_to).await?;
                }
            }
            Err(e) => {
                tracing::error!("TTS failed: {}", e);
                self.send_message(chat_id, text, reply_to).await?;
            }
        }
        Ok(())
    }

    /// Process a single update.
    async fn handle_update(&self, update: &serde_json::Value) -> anyhow::Result<()> {
        // Handle callback queries (inline button presses)
        if let Some(callback) = update.get("callback_query") {
            return self.handle_callback_query(callback).await;
        }
        
        let message = match update.get("message") {
            Some(m) => m,
            None => return Ok(()), // Skip non-message updates
        };

        let chat_id = message["chat"]["id"].as_i64().unwrap_or(0);
        let message_id = message["message_id"].as_i64();
        let user_id = message["from"]["id"].as_i64().unwrap_or(0);
        let mut text = message["text"].as_str().unwrap_or("").to_string();
        let chat_type = message["chat"]["type"].as_str().unwrap_or("private");
        
        // Determine if this is a group chat
        let is_group = chat_type == "group" || chat_type == "supergroup";

        // Handle voice messages
        if let Some(voice) = message.get("voice") {
            // For groups, only handle voice if policy allows
            if is_group && !self.should_respond_in_group(&text) {
                return Ok(());
            }
            return self.handle_voice_message(chat_id, user_id, voice, is_group.then_some(message_id).flatten()).await;
        }
        
        // Handle document uploads
        if let Some(document) = message.get("document") {
            let reply_to = if is_group { message_id } else { None };
            return self.handle_document(chat_id, user_id, document, reply_to).await;
        }
        
        // Handle photo uploads (get largest size)
        if let Some(photos) = message.get("photo").and_then(|p| p.as_array()) {
            if let Some(largest) = photos.last() {
                let reply_to = if is_group { message_id } else { None };
                return self.handle_photo(chat_id, user_id, largest, reply_to).await;
            }
        }

        if text.is_empty() {
            // Check for audio (not voice note)
            if message.get("audio").is_some() {
                self.send_message(chat_id, "🎵 Audio files not yet supported. Please send a voice message.", None).await?;
                return Ok(());
            }
            return Ok(());
        }

        // Handle group chat policy
        if is_group {
            match self.config.group_policy.as_str() {
                "off" => {
                    tracing::debug!("Ignoring group message (policy: off)");
                    return Ok(());
                }
                "mention" => {
                    // Only respond if bot is @mentioned
                    let mention = format!("@{}", self.bot_username);
                    if !text.contains(&mention) {
                        tracing::debug!("Ignoring group message (no mention)");
                        return Ok(());
                    }
                    // Strip the @mention from the message
                    text = text.replace(&mention, "").trim().to_string();
                    if text.is_empty() {
                        // Just a mention with no text
                        self.send_message(chat_id, "Yes?", message_id).await?;
                        return Ok(());
                    }
                }
                "open" => {
                    // Respond to all messages
                }
                other => {
                    tracing::warn!("Unknown group_policy: {}, defaulting to 'mention'", other);
                    let mention = format!("@{}", self.bot_username);
                    if !text.contains(&mention) {
                        return Ok(());
                    }
                    text = text.replace(&mention, "").trim().to_string();
                }
            }
        }

        // Check access
        if !self.config.allowed_users.is_empty()
            && !self.config.allowed_users.contains(&user_id)
        {
            tracing::warn!("Unauthorized user: {}", user_id);
            return Ok(());
        }

        // Handle slash commands before passing to agent
        if text.starts_with('/') {
            let handled = self.handle_command(chat_id, &text).await?;
            if handled {
                return Ok(());
            }
        }

        // Build session key
        let session_key = format!("telegram:{}", chat_id);
        
        // In groups, reply to the user's message
        let reply_to = if is_group { message_id } else { None };

        tracing::info!("Message from user {} in chat {}: {}", user_id, chat_id, 
            text_utils::truncate_chars(&text, 50));

        // Check for voice mode toggle
        if let Some(enabled) = Self::detect_voice_mode_toggle(&text) {
            self.runner.voice_mode.set(chat_id, enabled).await;
            let msg = if enabled {
                "🎙 Voice mode ON — 接下来我会用语音回复你"
            } else {
                "💬 Voice mode OFF — 切换回文字回复"
            };
            self.send_message(chat_id, msg, reply_to).await?;
            return Ok(());
        }

        // Build structured message context
        let msg_ctx = crate::context::MessageContext {
            sender_id: Some(user_id.to_string()),
            sender_name: message["from"]["first_name"].as_str().map(String::from),
            sender_username: message["from"]["username"].as_str().map(String::from),
            chat_type: if is_group {
                crate::context::ChatType::Group {
                    title: message["chat"]["title"].as_str().map(String::from),
                }
            } else {
                crate::context::ChatType::Direct
            },
            reply_to: message.get("reply_to_message").and_then(|m| {
                Some(crate::context::QuotedMessage {
                    text: m["text"].as_str().unwrap_or("").to_string(),
                    sender_name: m["from"]["first_name"].as_str().map(String::from),
                    message_id: m["message_id"].as_i64(),
                })
            }),
            message_id,
        };

        // Send persistent "typing" indicator that repeats every 4 seconds
        let typing_client = self.client.clone();
        let typing_url = self.api_url("sendChatAction");
        let typing_chat_id = chat_id;
        let typing_handle = tokio::spawn(async move {
            loop {
                let _ = typing_client
                    .post(&typing_url)
                    .json(&serde_json::json!({
                        "chat_id": typing_chat_id,
                        "action": "typing",
                    }))
                    .send()
                    .await;
                tokio::time::sleep(std::time::Duration::from_secs(4)).await;
            }
        });

        // Process with agent (streaming or regular)
        let result = if self.config.stream_mode {
            self.process_with_streaming(chat_id, &session_key, &text, user_id, reply_to).await
        } else {
            match self
                .runner
                .process_message_with_context(&session_key, &text, &msg_ctx, false)
                .await
            {
                Ok(response) => {
                    typing_handle.abort();
                    if response.is_silent {
                        Ok(())
                    } else {
                        let effective_reply = response.reply_to.or(reply_to);
                        // Voice decided purely by voice mode state
                        if self.runner.voice_mode.is_enabled(chat_id).await {
                            self.send_as_voice(chat_id, &response.text, effective_reply).await
                        } else {
                            self.send_message(chat_id, &response.text, effective_reply).await
                        }
                    }
                }
                Err(e) => {
                    typing_handle.abort();
                    tracing::error!("Agent error: {}", e);
                    self.send_message(chat_id, &format!("⚠️ Error: {}", e), reply_to).await
                }
            }
        };
        
        // Ensure typing is stopped
        typing_handle.abort();
        result?;

        Ok(())
    }
    
    /// Check if bot should respond in a group based on policy.
    fn should_respond_in_group(&self, text: &str) -> bool {
        match self.config.group_policy.as_str() {
            "off" => false,
            "open" => true,
            "mention" | _ => {
                let mention = format!("@{}", self.bot_username);
                text.contains(&mention)
            }
        }
    }
    
    /// Available models for model switching.
    const AVAILABLE_MODELS: &'static [(&'static str, &'static str)] = &[
        ("claude-sonnet-4-5-20250929", "Sonnet 4.5"),
        ("claude-sonnet-4-6", "Sonnet 4.6"),
        ("claude-opus-4-5-20251101", "Opus 4.5"),
        ("claude-opus-4-6", "Opus 4.6"),
        ("claude-haiku-4-5-20251001", "Haiku 4.5"),
    ];

    /// Handle slash commands. Returns true if the command was handled.
    async fn handle_command(&self, chat_id: i64, text: &str) -> anyhow::Result<bool> {
        let cmd = text.split_whitespace().next().unwrap_or("");
        // Strip @botname from command (e.g., /model@rustblawbot)
        let cmd = cmd.split('@').next().unwrap_or(cmd);

        match cmd {
            "/model" => {
                let arg = text.strip_prefix(cmd).unwrap_or("").trim();
                if arg.is_empty() {
                    // Show model selection with inline keyboard
                    let current = self.runner.current_model().await;
                    let mut buttons = Vec::new();
                    for (model_id, label) in Self::AVAILABLE_MODELS {
                        let marker = if current == *model_id { " ✓" } else { "" };
                        buttons.push(serde_json::json!([{
                            "text": format!("{}{}", label, marker),
                            "callback_data": format!("__model:{}", model_id)
                        }]));
                    }
                    let payload = serde_json::json!({
                        "chat_id": chat_id,
                        "text": format!("🤖 Current model: `{}`\n\nChoose a model:", current),
                        "parse_mode": "Markdown",
                        "reply_markup": {
                            "inline_keyboard": buttons
                        }
                    });
                    self.client
                        .post(self.api_url("sendMessage"))
                        .json(&payload)
                        .send()
                        .await?;
                    return Ok(true);
                }
                // Direct model set: /model claude-sonnet-4-6
                self.runner.set_model(arg).await;
                self.send_message(chat_id, &format!("✅ Model set to `{}`", arg), None).await?;
                Ok(true)
            }
            "/status" => {
                let model = self.runner.current_model().await;
                let msg = format!(
                    "🐾 **RustClaw Status**\n\n\
                     • Model: `{}`\n\
                     • Auth: OAuth (Keychain)\n\
                     • Bot: @{}\n\
                     • Status: Online ✅",
                    model, self.bot_username
                );
                self.send_message(chat_id, &msg, None).await?;
                Ok(true)
            }
            "/new" => {
                let session_key = format!("telegram:{}", chat_id);
                self.runner.clear_session(&session_key).await;
                self.send_message(chat_id, "🔄 New conversation started.", None).await?;
                Ok(true)
            }
            "/help" => {
                let msg = "🐾 **RustClaw Commands**\n\n\
                    /model — Show or switch AI model\n\
                    /status — Show bot status\n\
                    /new — Start a new conversation\n\
                    /help — Show this help";
                self.send_message(chat_id, msg, None).await?;
                Ok(true)
            }
            _ => Ok(false), // Not a known command, pass to agent
        }
    }

    /// Handle an inline button callback query.
    async fn handle_callback_query(&self, callback: &serde_json::Value) -> anyhow::Result<()> {
        let callback_id = callback["id"].as_str().unwrap_or("");
        let user_id = callback["from"]["id"].as_i64().unwrap_or(0);
        let data = callback["data"].as_str().unwrap_or("");
        
        // Get chat_id from the message the button was attached to
        let chat_id = callback["message"]["chat"]["id"].as_i64().unwrap_or(0);
        let message_id = callback["message"]["message_id"].as_i64();
        
        tracing::info!("Callback query from user {}: {}", user_id, data);
        
        // Check access
        if !self.config.allowed_users.is_empty()
            && !self.config.allowed_users.contains(&user_id)
        {
            tracing::warn!("Unauthorized user for callback: {}", user_id);
            self.answer_callback_query(callback_id, Some("Unauthorized")).await?;
            return Ok(());
        }

        // Handle model switch callbacks
        if let Some(model_id) = data.strip_prefix("__model:") {
            self.runner.set_model(model_id).await;
            // Find the display name
            let display = Self::AVAILABLE_MODELS.iter()
                .find(|(id, _)| *id == model_id)
                .map(|(_, name)| *name)
                .unwrap_or(model_id);
            self.answer_callback_query(callback_id, Some(&format!("Switched to {}", display))).await?;
            
            // Update the message to show the new selection
            if let Some(msg_id) = message_id {
                let mut buttons = Vec::new();
                for (mid, label) in Self::AVAILABLE_MODELS {
                    let marker = if *mid == model_id { " ✓" } else { "" };
                    buttons.push(serde_json::json!([{
                        "text": format!("{}{}", label, marker),
                        "callback_data": format!("__model:{}", mid)
                    }]));
                }
                let _ = self.client
                    .post(self.api_url("editMessageText"))
                    .json(&serde_json::json!({
                        "chat_id": chat_id,
                        "message_id": msg_id,
                        "text": format!("✅ Model: `{}`", model_id),
                        "parse_mode": "Markdown",
                        "reply_markup": { "inline_keyboard": buttons }
                    }))
                    .send()
                    .await;
            }
            return Ok(());
        }

        // Answer the callback query (removes the loading indicator)
        self.answer_callback_query(callback_id, None).await?;
        
        // Process callback_data as a new message
        let session_key = format!("telegram:{}", chat_id);
        let user_id_str = user_id.to_string();
        
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
        
        match self
            .runner
            .process_message(&session_key, data, Some(&user_id_str), Some("telegram"))
            .await
        {
            Ok(response) => {
                let trimmed = response.trim();
                if !trimmed.is_empty()
                    && trimmed != "NO_REPLY"
                    && trimmed != "HEARTBEAT_OK"
                {
                    self.send_message(chat_id, trimmed, message_id).await?;
                }
            }
            Err(e) => {
                tracing::error!("Agent error on callback: {}", e);
                self.send_message(chat_id, &format!("⚠️ Error: {}", e), message_id).await?;
            }
        }
        
        Ok(())
    }
    
    /// Answer a callback query (acknowledge button press).
    async fn answer_callback_query(&self, callback_id: &str, text: Option<&str>) -> anyhow::Result<()> {
        let mut payload = serde_json::json!({
            "callback_query_id": callback_id,
        });
        if let Some(t) = text {
            payload["text"] = serde_json::json!(t);
        }
        
        self.client
            .post(self.api_url("answerCallbackQuery"))
            .json(&payload)
            .send()
            .await?;
        
        Ok(())
    }
    
    /// Handle an incoming document.
    async fn handle_document(
        &self,
        chat_id: i64,
        user_id: i64,
        document: &serde_json::Value,
        reply_to: Option<i64>,
    ) -> anyhow::Result<()> {
        // Check access
        if !self.config.allowed_users.is_empty()
            && !self.config.allowed_users.contains(&user_id)
        {
            tracing::warn!("Unauthorized user for document: {}", user_id);
            return Ok(());
        }
        
        let file_id = document["file_id"].as_str().unwrap_or("");
        let file_name = document["file_name"].as_str().unwrap_or("unknown_file");
        
        tracing::info!("Document from user {}: {}", user_id, file_name);
        
        // Download the file
        match self.download_telegram_file(file_id, file_name).await {
            Ok(saved_path) => {
                let message = format!("[File received: {}, saved to {}]", file_name, saved_path);
                let session_key = format!("telegram:{}", chat_id);
                let user_id_str = user_id.to_string();
                
                // Process through agent
                match self
                    .runner
                    .process_message(&session_key, &message, Some(&user_id_str), Some("telegram"))
                    .await
                {
                    Ok(response) => {
                        let trimmed = response.trim();
                        if !trimmed.is_empty() && trimmed != "NO_REPLY" && trimmed != "HEARTBEAT_OK" {
                            self.send_response_with_files(chat_id, trimmed, reply_to).await?;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Agent error on document: {}", e);
                        self.send_message(chat_id, &format!("⚠️ Error: {}", e), reply_to).await?;
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to download document: {}", e);
                self.send_message(chat_id, &format!("⚠️ Failed to download file: {}", e), reply_to).await?;
            }
        }
        
        Ok(())
    }
    
    /// Handle an incoming photo.
    async fn handle_photo(
        &self,
        chat_id: i64,
        user_id: i64,
        photo: &serde_json::Value,
        reply_to: Option<i64>,
    ) -> anyhow::Result<()> {
        // Check access
        if !self.config.allowed_users.is_empty()
            && !self.config.allowed_users.contains(&user_id)
        {
            tracing::warn!("Unauthorized user for photo: {}", user_id);
            return Ok(());
        }
        
        let file_id = photo["file_id"].as_str().unwrap_or("");
        let file_unique_id = photo["file_unique_id"].as_str().unwrap_or("photo");
        let file_name = format!("{}.jpg", file_unique_id);
        
        tracing::info!("Photo from user {}", user_id);
        
        // Download the file
        match self.download_telegram_file(file_id, &file_name).await {
            Ok(saved_path) => {
                let message = format!("[Photo received, saved to {}]", saved_path);
                let session_key = format!("telegram:{}", chat_id);
                let user_id_str = user_id.to_string();
                
                // Process through agent
                match self
                    .runner
                    .process_message(&session_key, &message, Some(&user_id_str), Some("telegram"))
                    .await
                {
                    Ok(response) => {
                        let trimmed = response.trim();
                        if !trimmed.is_empty() && trimmed != "NO_REPLY" && trimmed != "HEARTBEAT_OK" {
                            self.send_response_with_files(chat_id, trimmed, reply_to).await?;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Agent error on photo: {}", e);
                        self.send_message(chat_id, &format!("⚠️ Error: {}", e), reply_to).await?;
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to download photo: {}", e);
                self.send_message(chat_id, &format!("⚠️ Failed to download photo: {}", e), reply_to).await?;
            }
        }
        
        Ok(())
    }
    
    /// Download a file from Telegram and save it locally.
    async fn download_telegram_file(&self, file_id: &str, file_name: &str) -> anyhow::Result<String> {
        // Ensure directory exists
        let dir = "/tmp/rustclaw_files";
        tokio::fs::create_dir_all(dir).await?;
        
        // Get file path via getFile API
        let file_info = self
            .client
            .post(self.api_url("getFile"))
            .json(&serde_json::json!({ "file_id": file_id }))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        
        let file_path = file_info["result"]["file_path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Could not get file path from Telegram"))?;
        
        // Download the file
        let download_url = format!("{}/file/bot{}/{}", TELEGRAM_API, self.token, file_path);
        let file_bytes = self
            .client
            .get(&download_url)
            .send()
            .await?
            .bytes()
            .await?;
        
        // Save to local file
        let saved_path = format!("{}/{}", dir, file_name);
        tokio::fs::write(&saved_path, &file_bytes).await?;
        
        tracing::debug!("Downloaded file to {}", saved_path);
        Ok(saved_path)
    }
    
    /// Send a document to a chat.
    async fn send_document(&self, chat_id: i64, file_path: &str, reply_to: Option<i64>) -> anyhow::Result<()> {
        let file_bytes = tokio::fs::read(file_path).await?;
        let file_name = std::path::Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");
        
        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(file_name.to_string());
        
        let mut form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part("document", part);
        
        if let Some(msg_id) = reply_to {
            form = form.text("reply_to_message_id", msg_id.to_string());
        }
        
        self.client
            .post(self.api_url("sendDocument"))
            .multipart(form)
            .send()
            .await?;
        
        Ok(())
    }
    
    /// Send a response, checking for FILE: patterns to send as documents.
    async fn send_response_with_files(&self, chat_id: i64, response: &str, reply_to: Option<i64>) -> anyhow::Result<()> {
        // Check for FILE: patterns
        let file_re = regex::Regex::new(r"FILE:(/[^\s]+)").unwrap();
        let mut text_without_files = response.to_string();
        let mut files_to_send: Vec<String> = Vec::new();
        
        for cap in file_re.captures_iter(response) {
            let file_path = cap[1].to_string();
            files_to_send.push(file_path.clone());
            text_without_files = text_without_files.replace(&format!("FILE:{}", file_path), "");
        }
        
        // Send text message first (if there's text left)
        // Use send_response to handle voice mode / VOICE: prefix
        let clean_text = text_without_files.trim();
        if !clean_text.is_empty() {
            self.send_response(chat_id, clean_text, reply_to).await?;
        }
        
        // Send files
        for file_path in files_to_send {
            if std::path::Path::new(&file_path).exists() {
                if let Err(e) = self.send_document(chat_id, &file_path, None).await {
                    tracing::error!("Failed to send file {}: {}", file_path, e);
                    self.send_message(chat_id, &format!("⚠️ Failed to send file: {}", file_path), None).await?;
                }
            } else {
                tracing::warn!("File not found: {}", file_path);
                self.send_message(chat_id, &format!("⚠️ File not found: {}", file_path), None).await?;
            }
        }
        
        Ok(())
    }

    /// Extract voice text if response should be sent as voice.
    /// Returns None if not a voice response, Some(text) with prefix stripped otherwise.
    /// Handles VOICE: at the start or after some preamble text.
    /// Send a response, automatically using voice if voice mode is on or VOICE: prefix detected.
    async fn send_response(&self, chat_id: i64, response: &str, reply_to: Option<i64>) -> anyhow::Result<()> {
        let trimmed = response.trim();
        if trimmed.is_empty() || trimmed == "NO_REPLY" || trimmed == "HEARTBEAT_OK" {
            return Ok(());
        }

        if self.runner.voice_mode.is_enabled(chat_id).await {
            self.send_as_voice(chat_id, trimmed, reply_to).await
        } else {
            self.send_message(chat_id, trimmed, reply_to).await
        }
    }

    /// Check if user message is toggling voice mode. Returns Some(true/false) if toggling.
    fn detect_voice_mode_toggle(text: &str) -> Option<bool> {
        let lower = text.to_lowercase();
        let normalized = lower.trim();

        // Fast-path patterns for unambiguous exact phrases only.
        // For anything ambiguous (e.g. "关闭语音模式" contains "语音模式"),
        // let the LLM handle it via set_voice_mode tool.
        let disable_patterns = [
            "text mode", "文字模式", "关闭语音", "停止语音", "不要语音",
        ];
        let enable_patterns = [
            "开启语音", "打开语音", "voice mode on", "start voice",
        ];

        // Check disable first (higher priority)
        for p in &disable_patterns {
            if normalized.contains(p) {
                return Some(false);
            }
        }
        for p in &enable_patterns {
            if normalized.contains(p) {
                return Some(true);
            }
        }
        // "voice mode" / "语音模式" alone is ambiguous — let LLM decide
        None
    }

    /// Handle a voice message by downloading, transcribing, and processing.
    async fn handle_voice_message(
        &self,
        chat_id: i64,
        user_id: i64,
        voice: &serde_json::Value,
        reply_to: Option<i64>,
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
                self.send_message(chat_id, "⚠️ Could not get voice file ID", reply_to).await?;
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
                self.send_message(chat_id, "⚠️ Could not get file path from Telegram", reply_to).await?;
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
        tracing::info!("Transcribed: {}", { let _end = transcription.len().min(50); let _end = transcription.floor_char_boundary(_end); &transcription[.._end] });

        // Clean up the input file
        let _ = tokio::fs::remove_file(ogg_path).await;

        // Step 4: Check for voice mode toggle in transcription
        if let Some(enabled) = Self::detect_voice_mode_toggle(&transcription) {
            self.runner.voice_mode.set(chat_id, enabled).await;
            let msg = if enabled {
                "🎙 Voice mode ON — 接下来我会用语音回复你"
            } else {
                "💬 Voice mode OFF — 切换回文字回复"
            };
            if enabled {
                self.send_as_voice(chat_id, msg, reply_to).await?;
            } else {
                self.send_message(chat_id, msg, reply_to).await?;
            }
            return Ok(());
        }

        // Step 5: Process through agent with [Voice message] prefix
        let user_message = format!("[Voice message] {}", transcription);
        let session_key = format!("telegram:{}", chat_id);

        let msg_ctx = crate::context::MessageContext {
            sender_id: Some(user_id.to_string()),
            chat_type: crate::context::ChatType::Direct,
            ..Default::default()
        };

        match self
            .runner
            .process_message_with_context(&session_key, &user_message, &msg_ctx, false)
            .await
        {
            Ok(response) => {
                if response.is_silent {
                    return Ok(());
                }
                let effective_reply = response.reply_to.or(reply_to);
                if self.runner.voice_mode.is_enabled(chat_id).await {
                    self.send_as_voice(chat_id, &response.text, effective_reply).await?;
                } else {
                    self.send_message(chat_id, &response.text, effective_reply).await?;
                }
            }
            Err(e) => {
                tracing::error!("Agent error: {}", e);
                self.send_message(chat_id, &format!("⚠️ Error: {}", e), reply_to).await?;
            }
        }

        Ok(())
    }

    /// Send a message and return the message ID.
    async fn send_message_get_id(&self, chat_id: i64, text: &str) -> anyhow::Result<i64> {
        let response = self.client
            .post(self.api_url("sendMessage"))
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "text": text,
            }))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        let message_id = response["result"]["message_id"]
            .as_i64()
            .ok_or_else(|| anyhow::anyhow!("No message_id in response"))?;

        Ok(message_id)
    }

    /// Edit an existing message.
    async fn edit_message(&self, chat_id: i64, message_id: i64, text: &str) -> anyhow::Result<()> {
        // Try MarkdownV2 first
        let escaped = escape_markdown_v2(text);
        let response = self.client
            .post(self.api_url("editMessageText"))
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "message_id": message_id,
                "text": escaped,
                "parse_mode": "MarkdownV2",
            }))
            .send()
            .await?;

        // If MarkdownV2 fails, retry without parse_mode
        if response.status().as_u16() == 400 {
            self.client
                .post(self.api_url("editMessageText"))
                .json(&serde_json::json!({
                    "chat_id": chat_id,
                    "message_id": message_id,
                    "text": text,
                }))
                .send()
                .await?;
        }

        Ok(())
    }

    /// Process a message with streaming response.
    /// Sends initial message, then edits it as chunks arrive.
    #[allow(dead_code)]
    async fn process_with_streaming(
        &self,
        chat_id: i64,
        session_key: &str,
        user_message: &str,
        user_id: i64,
        reply_to: Option<i64>,
    ) -> anyhow::Result<()> {
        let user_id_str = user_id.to_string();

        // Get streaming response
        let mut stream_rx = self
            .runner
            .process_message_streaming(session_key, user_message, Some(&user_id_str), Some("telegram"))
            .await?;

        let mut full_response = String::new();
        let mut message_id: Option<i64> = None;
        let mut last_update = std::time::Instant::now();
        let update_interval = std::time::Duration::from_millis(500);
        let min_chars_for_update = 100;

        while let Some(chunk) = stream_rx.recv().await {
            full_response.push_str(&chunk);

            // Batch updates: every 500ms or every 100 chars
            let should_update = last_update.elapsed() >= update_interval
                || full_response.len() % min_chars_for_update == 0;

            if should_update && !full_response.is_empty() {
                match message_id {
                    None => {
                        // Send initial message
                        let display_text = if full_response.len() > 50 {
                            format!("{}...", &full_response)
                        } else {
                            full_response.clone()
                        };
                        match self.send_message_get_id(chat_id, &display_text).await {
                            Ok(id) => {
                                message_id = Some(id);
                                last_update = std::time::Instant::now();
                            }
                            Err(e) => {
                                tracing::warn!("Failed to send initial streaming message: {}", e);
                            }
                        }
                    }
                    Some(msg_id) => {
                        // Edit existing message
                        if let Err(e) = self.edit_message(chat_id, msg_id, &full_response).await {
                            tracing::debug!("Failed to edit message: {}", e);
                        }
                        last_update = std::time::Instant::now();
                    }
                }
            }
        }

        // Final update with complete response
        let trimmed = full_response.trim();
        if !trimmed.is_empty() && trimmed != "NO_REPLY" && trimmed != "HEARTBEAT_OK" {
            match message_id {
                Some(msg_id) => {
                    // Edit with final content
                    if let Err(e) = self.edit_message(chat_id, msg_id, trimmed).await {
                        tracing::warn!("Failed to edit final message: {}", e);
                    }
                }
                None => {
                    // Never sent any message, send now
                    self.send_message(chat_id, trimmed, reply_to).await?;
                }
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
                    "allowed_updates": ["message", "callback_query"],
                }))
                .send()
                .await;

            match resp {
                Ok(r) => {
                    let body: serde_json::Value = match r.json().await {
                        Ok(b) => b,
                        Err(e) => {
                            tracing::error!("Failed to parse Telegram response: {}. Retrying in 5s...", e);
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                            continue;
                        }
                    };
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

/// Inline button parsed from text.
struct InlineButton {
    text: String,
    callback_data: String,
}

/// Extract inline buttons from text.
/// Pattern: `[[button:text|callback_data]]`
/// Returns (clean_text, buttons)
fn extract_inline_buttons(text: &str) -> (String, Vec<InlineButton>) {
    let mut buttons = Vec::new();
    let mut clean_text = text.to_string();
    
    // Pattern: [[button:text|callback_data]]
    let re = regex::Regex::new(r"\[\[button:([^|]+)\|([^\]]+)\]\]").unwrap();
    
    for cap in re.captures_iter(text) {
        buttons.push(InlineButton {
            text: cap[1].trim().to_string(),
            callback_data: cap[2].trim().to_string(),
        });
    }
    
    // Remove button patterns from text
    clean_text = re.replace_all(&clean_text, "").to_string();
    // Clean up extra whitespace
    clean_text = clean_text.trim().to_string();
    
    (clean_text, buttons)
}

/// Build Telegram inline keyboard JSON from buttons.
/// Places buttons in rows of up to 3 buttons each.
fn build_inline_keyboard(buttons: &[InlineButton]) -> serde_json::Value {
    let mut rows: Vec<Vec<serde_json::Value>> = Vec::new();
    let mut current_row: Vec<serde_json::Value> = Vec::new();
    
    for button in buttons {
        current_row.push(serde_json::json!({
            "text": button.text,
            "callback_data": button.callback_data,
        }));
        
        // Max 3 buttons per row
        if current_row.len() >= 3 {
            rows.push(current_row);
            current_row = Vec::new();
        }
    }
    
    // Add remaining buttons
    if !current_row.is_empty() {
        rows.push(current_row);
    }
    
    serde_json::json!({
        "inline_keyboard": rows
    })
}

/// Start the Telegram channel.
pub async fn start(config: TelegramConfig, runner: Arc<AgentRunner>) -> anyhow::Result<()> {
    let bot = TelegramBot::new(config, runner).await?;
    // Register channel capabilities with the agent runner
    bot.runner.set_channel_capabilities(bot.capabilities()).await;
    bot.run().await
}

/// Strip markdown formatting from text for TTS output.
/// Removes *, #, `, |, [], and other markdown symbols that sound unnatural when spoken.
fn strip_markdown(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for line in text.lines() {
        let trimmed = line.trim();
        // Skip pure markdown table separator lines (|---|---|)
        if trimmed.starts_with('|') && trimmed.contains("---") {
            continue;
        }
        // Remove heading markers
        let line = if trimmed.starts_with('#') {
            trimmed.trim_start_matches('#').trim()
        } else {
            trimmed
        };
        // Remove table pipes at start/end
        let line = line.trim_start_matches('|').trim_end_matches('|');
        result.push_str(line);
        result.push('\n');
    }
    // Remove bold/italic markers
    let result = result.replace("**", "").replace("__", "");
    // Remove inline code
    let result = result.replace('`', "");
    // Remove link syntax [text](url) → text
    let re = regex::Regex::new(r"\[([^\]]+)\]\([^)]+\)").unwrap();
    let result = re.replace_all(&result, "$1").to_string();
    // Remove remaining pipes (table cells)
    let result = result.replace(" | ", ", ");
    // Clean up extra whitespace
    let result = result.replace("  ", " ");
    result.trim().to_string()
}
