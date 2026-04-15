//! Telegram channel adapter using raw HTTP API.
//!
//! Uses long polling (getUpdates) — simple, no webhook needed.

use std::sync::Arc;

use crate::agent::AgentRunner;
use crate::config::TelegramConfig;
use crate::stt;
use crate::text_utils;


const TELEGRAM_API: &str = "https://api.telegram.org";

/// RAII guard that removes a session key from active_sessions on drop.
/// Guarantees cleanup even if the handler panics or returns early.
struct ActiveSessionGuard {
    active_sessions: Arc<tokio::sync::Mutex<std::collections::HashSet<String>>>,
    key: String,
}

impl Drop for ActiveSessionGuard {
    fn drop(&mut self) {
        // try_lock to avoid blocking in drop; if contended, spawn async cleanup
        if let Ok(mut active) = self.active_sessions.try_lock() {
            active.remove(&self.key);
        } else {
            let active_sessions = self.active_sessions.clone();
            let key = std::mem::take(&mut self.key);
            tokio::spawn(async move {
                active_sessions.lock().await.remove(&key);
            });
        }
    }
}

/// Telegram bot client.
#[derive(Clone)]
struct TelegramBot {
    client: reqwest::Client,
    token: String,
    config: TelegramConfig,
    runner: Arc<AgentRunner>,
    /// Bot username (fetched via getMe on startup)
    bot_username: String,
    /// Sessions currently being processed (for queue routing)
    active_sessions: Arc<tokio::sync::Mutex<std::collections::HashSet<String>>>,
    /// Shared cancel registry for running rituals
    ritual_cancel_registry: crate::ritual_runner::CancelRegistry,
    /// Shared event registry for sending events to paused rituals
    ritual_event_registry: crate::ritual_runner::EventRegistry,
    /// Active autopilot handle (if running)
    autopilot_handle: Arc<tokio::sync::Mutex<Option<crate::autopilot::AutopilotHandle>>>,
    /// Generation counter for autopilot resume timers (prevents stacking)
    autopilot_resume_gen: Arc<std::sync::atomic::AtomicU64>,
    /// Pending ritual tasks waiting for project selection (chat_id → task description)
    pending_ritual_tasks: Arc<tokio::sync::Mutex<std::collections::HashMap<i64, String>>>,
}

impl TelegramBot {
    async fn new(config: TelegramConfig, runner: Arc<AgentRunner>) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(45))
            .connect_timeout(std::time::Duration::from_secs(10))
            .tcp_keepalive(std::time::Duration::from_secs(15))
            .build()?;
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
            active_sessions: Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::new())),
            ritual_cancel_registry: crate::ritual_runner::new_cancel_registry(),
            ritual_event_registry: crate::ritual_runner::new_event_registry(),
            autopilot_handle: Arc::new(tokio::sync::Mutex::new(None)),
            autopilot_resume_gen: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            pending_ritual_tasks: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
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

    /// Send a text message with Telegram HTML formatting.
    /// Markdown from LLM output is converted to Telegram HTML via pulldown-cmark.
    /// Falls back to stripped plain text if HTML parse fails.
    /// Parses `[[button:text|callback_data]]` patterns into inline keyboard buttons.
    async fn send_message(&self, chat_id: i64, text: &str, reply_to: Option<i64>) -> anyhow::Result<()> {
        // Extract inline buttons from text
        let (clean_text, buttons) = extract_inline_buttons(text);
        
        // Convert markdown → Telegram HTML
        let html_text = crate::markdown::to_telegram_html(&clean_text);
        
        // Split long messages (Telegram limit: 4096 chars)
        let chunks = text_utils::split_message(&html_text, 4096);
        let total_chunks = chunks.len();
        
        for (i, chunk) in chunks.iter().enumerate() {
            // Only reply to the first chunk
            let reply_id = if i == 0 { reply_to } else { None };
            // Only add buttons to the last chunk
            let add_buttons = i == total_chunks - 1 && !buttons.is_empty();
            
            let mut payload = serde_json::json!({
                "chat_id": chat_id,
                "text": chunk,
                "parse_mode": "HTML",
            });
            if let Some(msg_id) = reply_id {
                payload["reply_to_message_id"] = serde_json::json!(msg_id);
            }
            if add_buttons {
                payload["reply_markup"] = build_inline_keyboard(&buttons);
            }
            
            let resp = self.client
                .post(self.api_url("sendMessage"))
                .json(&payload)
                .send()
                .await?;
            
            // If HTML parse failed (400), fall back to stripped plain text
            if resp.status() == 400 {
                tracing::warn!("HTML parse failed, falling back to plain text");
                let plain = crate::markdown::strip_markdown(&clean_text);
                let plain_chunks = text_utils::split_message(&plain, 4096);
                let plain_chunk = plain_chunks.get(i).unwrap_or(chunk);
                payload["text"] = serde_json::json!(plain_chunk);
                payload.as_object_mut().unwrap().remove("parse_mode");
                self.client
                    .post(self.api_url("sendMessage"))
                    .json(&payload)
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

        // Set ritual notify for this request so start_ritual tool can send Telegram messages
        if let Ok(mut guard) = self.runner.tools.ritual_notify.lock() {
            *guard = Some(self.make_notify_fn(chat_id));
        }
        // Set current session key so fire-and-forget sub-agents can inject completion back
        let session_key = format!("telegram:{}", chat_id);
        if let Ok(mut guard) = self.runner.tools.current_session_key.lock() {
            *guard = Some(session_key.clone());
        }
        
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

        // Pause autopilot while user is chatting — resume after 60s idle
        // Uses generation counter so only the latest timer resumes
        {
            let guard = self.autopilot_handle.lock().await;
            if let Some(ref h) = *guard {
                if h.is_running() {
                    h.pause();
                    let gen = self.autopilot_resume_gen.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    let gen_ref = self.autopilot_resume_gen.clone();
                    let handle = h.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                        // Only resume if no newer timer was spawned
                        if gen_ref.load(std::sync::atomic::Ordering::Relaxed) == gen && handle.is_paused() {
                            handle.resume();
                            tracing::info!("Autopilot auto-resumed after 60s idle");
                        }
                    });
                }
            }
        }

        // In groups, reply to the user's message
        let reply_to = if is_group { message_id } else { None };

        tracing::info!("Message from user {} in chat {}: {}", user_id, chat_id, 
            text_utils::truncate_chars(&text, 50));

        // Voice mode toggle is handled entirely by LLM via set_voice_mode tool.
        // No hardcoded pattern matching — LLM understands any phrasing.

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
            reply_to: message.get("reply_to_message")
                .and_then(crate::context::QuotedMessage::from_telegram_json),
            message_id,
        };

        // Send persistent "typing" indicator that repeats every 4 seconds
        let typing_client = self.client.clone();
        let typing_url = self.api_url("sendChatAction");
        let typing_chat_id = chat_id;
        let mut typing_handle = tokio::spawn(async move {
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

        // Check if this session is already busy
        {
            let active = self.active_sessions.lock().await;
            if active.contains(&session_key) {
                // Session busy — check for /btw or queue the message
                typing_handle.abort();
                if text.starts_with("/btw ") || text.starts_with("/btw\n") {
                    let question = text.strip_prefix("/btw").unwrap().trim();
                    match self.runner.process_btw(&session_key, question, Some(&user_id.to_string()), Some("telegram")).await {
                        Ok(response) => {
                            let trimmed = response.trim();
                            if !trimmed.is_empty() && trimmed != "NO_REPLY" {
                                self.send_message(chat_id, trimmed, reply_to).await?;
                            }
                        }
                        Err(e) => {
                            self.send_message(chat_id, &format!("⚠️ BTW error: {}", e), reply_to).await?;
                        }
                    }
                } else if self.try_route_to_waiting_ritual(chat_id, &text).await {
                    // Message was routed to a waiting ritual — done
                    tracing::info!("Routed message to waiting ritual for chat {}", chat_id);
                } else {
                    // Queue the message for injection into the running session
                    self.runner.queue_message(
                        &session_key,
                        &text,
                        Some(&user_id.to_string()),
                        crate::message_queue::Priority::Next,
                    ).await;
                    tracing::info!("Queued message for busy session {}", session_key);
                }
                return Ok(());
            }
        }

        // Check for a waiting ritual even when session is idle (e.g. user types
        // plain text while the main agent is not running but a ritual is paused).
        typing_handle.abort();
        if self.try_route_to_waiting_ritual(chat_id, &text).await {
            tracing::info!("Routed message to waiting ritual (idle session) for chat {}", chat_id);
            return Ok(());
        }

        // Mark session as active (with drop guard to guarantee cleanup)
        {
            let mut active = self.active_sessions.lock().await;
            active.insert(session_key.clone());
        }
        let _session_guard = ActiveSessionGuard {
            active_sessions: self.active_sessions.clone(),
            key: session_key.clone(),
        };

        // Prepend message context as prefix
        let channel_caps = self.runner.channel_caps.read().await;
        let prefix = msg_ctx.format_prefix(&channel_caps.name);
        let full_message = if prefix.is_empty() {
            text.clone()
        } else {
            format!("{}{}", prefix, text)
        };
        drop(channel_caps);

        // Process with event stream (unified path for both streaming and regular)
        let mut rx = self.runner.process_message_events(
            &session_key,
            &full_message,
            Some(&user_id.to_string()),
            Some("telegram"),
            false,
        );

        // Consume events
        let mut final_response = String::new();
        let mut had_error = false;

        while let Some(event) = rx.recv().await {
            use crate::events::AgentEvent;
            match event {
                AgentEvent::Text(text) => {
                    // Intermediate text — send immediately as acknowledgment
                    typing_handle.abort();
                    let response = crate::context::ProcessedResponse::from_raw(&text);
                    if !response.is_silent {
                        let effective_reply = response.reply_to.or(reply_to);
                        if self.runner.voice_mode.is_enabled(chat_id).await {
                            let _ = self.send_as_voice(chat_id, &response.text, effective_reply).await;
                        } else {
                            let _ = self.send_message(chat_id, &response.text, effective_reply).await;
                        }
                    }
                    // Restart typing indicator for tool execution
                    let typing_client = self.client.clone();
                    let typing_url = self.api_url("sendChatAction");
                    let typing_chat_id = chat_id;
                    typing_handle.abort();
                    typing_handle = tokio::spawn(async move {
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
                }
                AgentEvent::ToolStart { name, .. } => {
                    tracing::debug!("Tool starting: {}", name);
                    // Typing indicator already running
                }
                AgentEvent::ToolDone { name, is_error, .. } => {
                    tracing::debug!("Tool done: {} (error={})", name, is_error);
                }
                AgentEvent::Response(text) => {
                    final_response = text;
                }
                AgentEvent::Error(e) => {
                    tracing::error!("Agent error: {}", e);
                    had_error = true;
                    final_response = format!("⚠️ Error: {}", e);
                }
            }
        }

        // Stop typing
        typing_handle.abort();

        // session_guard drop will remove from active_sessions automatically

        // Send final response
        if !final_response.is_empty() {
            let response = crate::context::ProcessedResponse::from_raw(&final_response);
            if response.is_silent && !had_error {
                // Silent response (NO_REPLY, HEARTBEAT_OK)
            } else {
                let effective_reply = response.reply_to.or(reply_to);
                if self.runner.voice_mode.is_enabled(chat_id).await && !had_error {
                    self.send_as_voice(chat_id, &response.text, effective_reply).await?;
                } else {
                    self.send_message(chat_id, &response.text, effective_reply).await?;
                }
            }
        }

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
                        "text": format!("🤖 Current model: <code>{}</code>

Choose a model:", current),
                        "parse_mode": "HTML",
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
            "/stop" => {
                let args: Vec<&str> = text.split_whitespace().skip(1).collect();
                let session_key = format!("telegram:{}", chat_id);

                if let Some(task_id) = args.first() {
                    // /stop <task_id> — cancel specific sub-agent
                    let cancelled = self.runner.cancel_subagent(task_id).await;
                    if cancelled {
                        self.send_message(chat_id, &format!("⛔ Sub-agent `{}` stopped.", task_id), None).await?;
                    } else {
                        self.send_message(chat_id, &format!("Sub-agent `{}` not found.", task_id), None).await?;
                    }
                } else {
                    // /stop — cancel main session + all sub-agents + all running rituals
                    let main_cancelled = self.runner.cancel_session(&session_key).await;
                    let sub_count = self.runner.cancel_all_subagents().await;
                    let ritual_runner = self.make_ritual_runner(chat_id);
                    let ritual_count = ritual_runner.cancel_all_running();
                    // Also remove from active sessions so new messages aren't queued
                    {
                        let mut active = self.active_sessions.lock().await;
                        active.remove(&session_key);
                    }
                    let msg = match (main_cancelled, sub_count + ritual_count) {
                        (true, 0) => "⛔ Stopped.".to_string(),
                        (true, n) => format!("⛔ Stopped + cancelled {} task(s).", n),
                        (false, 0) => "Nothing running.".to_string(),
                        (false, n) => format!("⛔ Cancelled {} task(s).", n),
                    };
                    self.send_message(chat_id, &msg, None).await?;
                }
                Ok(true)
            }
            "/autopilot" => {
                let arg = text.strip_prefix("/autopilot").unwrap_or("").trim();
                let session_key = format!("telegram:{}", chat_id);
                let mut handle_guard = self.autopilot_handle.lock().await;

                if arg == "stop" || arg == "off" {
                    if let Some(h) = handle_guard.take() {
                        h.stop();
                        self.send_message(chat_id, "⛔ Autopilot stopped.", None).await?;
                    } else {
                        self.send_message(chat_id, "No autopilot running.", None).await?;
                    }
                    return Ok(true);
                }

                if arg == "status" {
                    if let Some(ref h) = *handle_guard {
                        if h.is_running() {
                            let (tasks, turns) = h.stats();
                            let tokens = h.total_tokens();
                            self.send_message(
                                chat_id,
                                &format!(
                                    "🤖 Autopilot is running{}.\n📊 {} tasks done, {} turns, {} tokens",
                                    if h.is_paused() { " (paused)" } else { "" },
                                    tasks, turns, tokens,
                                ),
                                None,
                            ).await?;
                        } else {
                            let (tasks, turns) = h.stats();
                            let tokens = h.total_tokens();
                            self.send_message(
                                chat_id,
                                &format!(
                                    "Autopilot finished.\n📊 {} tasks done, {} turns, {} tokens",
                                    tasks, turns, tokens,
                                ),
                                None,
                            ).await?;
                            *handle_guard = None;
                        }
                    } else {
                        self.send_message(chat_id, "No autopilot running.", None).await?;
                    }
                    return Ok(true);
                }

                // Check if already running
                if let Some(ref h) = *handle_guard {
                    if h.is_running() {
                        self.send_message(chat_id, "⚠️ Autopilot already running. Use `/autopilot stop` first.", None).await?;
                        return Ok(true);
                    }
                }

                let task_file = if arg.is_empty() { "HEARTBEAT.md" } else { arg };
                let workspace = self.runner.workspace_root().to_path_buf();
                let config = crate::autopilot::AutopilotConfig {
                    task_file: std::path::PathBuf::from(task_file),
                    max_turns_per_task: 60,
                    max_total_turns: 300,
                    session_key: session_key.clone(),
                };

                // Build notify function that sends progress to Telegram
                let tg_client = self.client.clone();
                let tg_token = self.token.clone();
                let tg_chat_id = chat_id;
                let notify_fn: Box<dyn Fn(&str) + Send + Sync + 'static> = Box::new(move |msg: &str| {
                    let client = tg_client.clone();
                    let token = tg_token.clone();
                    let text = format!("🤖 {}", msg);
                    tokio::spawn(async move {
                        let url = format!("https://api.telegram.org/bot{}/sendMessage", token);
                        let _ = client.post(&url)
                            .json(&serde_json::json!({"chat_id": tg_chat_id, "text": text}))
                            .send().await;
                    });
                });

                match crate::autopilot::run(self.runner.clone(), config, &workspace, Some(notify_fn)).await {
                    Ok((handle, join)) => {
                        *handle_guard = Some(handle);
                        // Monitor the join handle for panics/errors
                        tokio::spawn(async move {
                            match join.await {
                                Ok(Ok(count)) => tracing::info!("Autopilot finished: {} tasks completed", count),
                                Ok(Err(e)) => tracing::error!("Autopilot error: {}", e),
                                Err(e) => tracing::error!("Autopilot task panicked: {}", e),
                            }
                        });
                        self.send_message(
                            chat_id,
                            &format!("🤖 Autopilot started on `{}`", task_file),
                            None,
                        ).await?;
                    }
                    Err(e) => {
                        self.send_message(
                            chat_id,
                            &format!("❌ Autopilot error: {}", e),
                            None,
                        ).await?;
                    }
                }
                Ok(true)
            }
            "/sessions" => {
                let summaries = self.runner.sessions().list_session_summaries(5).await;
                if summaries.is_empty() {
                    self.send_message(chat_id, "No active sessions.", None).await?;
                } else {
                    let mut msg = String::from("📋 **Recent Sessions**\n\n");
                    for (i, s) in summaries.iter().enumerate() {
                        let time = if s.updated_at.len() >= 19 {
                            &s.updated_at[..19]
                        } else {
                            &s.updated_at
                        };
                        msg.push_str(&format!(
                            "{}. `{}`\n   📨 {} msgs • 🕐 {}\n\n",
                            i + 1,
                            s.key,
                            s.message_count,
                            time.replace('T', " "),
                        ));
                    }
                    self.send_message(chat_id, msg.trim(), None).await?;
                }
                Ok(true)
            }
            "/help" => {
                let msg = "🐾 **RustClaw Commands**\n\n\
                    /model — Show or switch AI model\n\
                    /status — Show bot status\n\
                    /new — Start a new conversation\n\
                    /stop — Stop current task\n\
                    /sessions — List recent active sessions\n\
                    /ritual — Run a development ritual (multi-phase pipeline)\n\
                    /ping — Pong with current time\n\
                    /help — Show this help";
                self.send_message(chat_id, msg, None).await?;
                Ok(true)
            }
            "/ritual" => {
                let arg = text.strip_prefix("/ritual").unwrap_or("").trim();
                // Strip @botname if present (e.g. /ritual@mybotname task)
                let arg = if arg.starts_with('@') {
                    arg.split_once(' ').map(|(_, rest)| rest.trim()).unwrap_or("")
                } else {
                    arg
                };
                self.handle_ritual_command(chat_id, arg).await?;
                Ok(true)
            }
            "/ping" => {
                let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
                self.send_message(chat_id, &format!("pong 🏓 {}", now), None).await?;
                Ok(true)
            }
            _ => Ok(false), // Not a known command, pass to agent
        }
    }

    /// Handle /ritual subcommands.
    async fn handle_ritual_command(&self, chat_id: i64, arg: &str) -> anyhow::Result<()> {
        use gid_core::ritual::{
            V2Event as RitualEvent,
        };

        match arg {
            "status" => {
                let runner = self.make_ritual_runner(chat_id);
                match runner.list_rituals() {
                    Ok(rituals) if rituals.is_empty() => {
                        self.send_message(chat_id, "No rituals found.", None).await?;
                    }
                    Ok(rituals) => {
                        let mut msg = String::from("📊 **Ritual Status**\n");
                        for state in rituals.iter().take(5) {
                            msg.push_str(&format!(
                                "\n`{}` — **{}** | `{}`\n  Task: {}\n  Updated: {}",
                                state.id,
                                state.phase.display_name(),
                                if state.phase.is_terminal() { "✅" } else if state.phase.is_paused() { "⏸" } else { "🔄" },
                                gid_core::ritual::truncate(&state.task, 60),
                                state.updated_at.format("%H:%M:%S UTC"),
                            ));
                            if !state.phase_tokens.is_empty() {
                                let total: u64 = state.phase_tokens.values().sum();
                                msg.push_str(&format!(" | 🪙 {}", format_token_count(total)));
                            }
                        }
                        if rituals.len() > 5 {
                            msg.push_str(&format!("\n\n...and {} more", rituals.len() - 5));
                        }
                        self.send_message(chat_id, &msg, None).await?;
                    }
                    Err(e) => {
                        self.send_message(chat_id, &format!("⚠️ Failed to list rituals: {}", e), None).await?;
                    }
                }
            }
            arg if arg == "cancel" || arg.starts_with("cancel ") => {
                let runner = self.make_ritual_runner(chat_id);
                let specific_id = arg.strip_prefix("cancel ").map(|s| s.trim()).filter(|s| !s.is_empty());

                let target_state = if let Some(id) = specific_id {
                    // Cancel specific ritual by ID
                    match runner.load_state_by_id(id) {
                        Ok(s) => s,
                        Err(_) => {
                            self.send_message(chat_id, &format!("❌ Ritual `{}` not found.", id), None).await?;
                            return Ok(());
                        }
                    }
                } else {
                    // Cancel latest active ritual
                    runner.load_state()?
                };

                if target_state.phase.is_terminal() || target_state.phase == gid_core::ritual::V2Phase::Idle {
                    self.send_message(chat_id, "No active ritual to cancel.", None).await?;
                } else {
                    // Immediately interrupt running ritual via cancellation token
                    let interrupted = runner.cancel_running(&target_state.id);
                    if interrupted {
                        tracing::info!(ritual_id = %target_state.id, "Interrupted running ritual via cancel token");
                        self.send_message(
                            chat_id,
                            &format!("🛑 Interrupting ritual `{}` (was in {} phase)...", target_state.id, target_state.phase.display_name()),
                            None,
                        ).await?;
                    } else {
                        // Ritual not actively running (paused/escalated) — update state directly
                        let cancelled = target_state.clone()
                            .with_phase(gid_core::ritual::V2Phase::Cancelled);
                        if let Err(e) = runner.save_state(&cancelled) {
                            tracing::error!("Failed to save cancelled state: {}", e);
                        }
                        self.send_message(
                            chat_id,
                            &format!("🛑 Ritual `{}` cancelled (was in {} phase).", target_state.id, target_state.phase.display_name()),
                            None,
                        ).await?;
                    }
                }
            }
            "retry" => {
                let runner = self.make_ritual_runner(chat_id);
                let state = runner.load_state()?;
                if state.phase != gid_core::ritual::V2Phase::Escalated
                    && state.phase != gid_core::ritual::V2Phase::WaitingClarification
                {
                    self.send_message(
                        chat_id,
                        "⚠️ Retry is only available when ritual is in Escalated or WaitingClarification state.",
                        None,
                    ).await?;
                } else {
                    self.send_message(chat_id, "🔄 Retrying ritual...", None).await?;
                    let bot = self.clone();
                    tokio::spawn(async move {
                        match runner.send_event(RitualEvent::UserRetry).await {
                            Ok(state) => {
                                let msg = format!("Ritual retry finished in {} phase.", state.phase.display_name());
                                let _ = bot.send_message(chat_id, &msg, None).await;
                            }
                            Err(e) => {
                                let _ = bot.send_message(chat_id, &format!("❌ Retry failed: {}", e), None).await;
                            }
                        }
                    });
                }
            }
            "skip" => {
                let runner = self.make_ritual_runner(chat_id);
                let state = runner.load_state()?;
                if state.phase.is_terminal() || state.phase == gid_core::ritual::V2Phase::Idle {
                    self.send_message(chat_id, "No active ritual to skip phase.", None).await?;
                } else {
                    self.send_message(
                        chat_id,
                        &format!("⏭️ Skipping {} phase...", state.phase.display_name()),
                        None,
                    ).await?;
                    let bot = self.clone();
                    tokio::spawn(async move {
                        match runner.send_event(RitualEvent::UserSkipPhase).await {
                            Ok(state) => {
                                let msg = format!("Skipped to {} phase.", state.phase.display_name());
                                let _ = bot.send_message(chat_id, &msg, None).await;
                            }
                            Err(e) => {
                                let _ = bot.send_message(chat_id, &format!("❌ Skip failed: {}", e), None).await;
                            }
                        }
                    });
                }
            }
            "done" => {
                let runner = self.make_ritual_runner(chat_id);
                let state = runner.load_state()?;
                if state.phase.is_terminal() || state.phase == gid_core::ritual::V2Phase::Idle {
                    self.send_message(chat_id, "No active ritual phase to mark as done.", None).await?;
                } else {
                    let phase_name = state.phase.display_name().to_string();
                    let ritual_id = state.id.clone();
                    self.send_message(
                        chat_id,
                        &format!("✅ Marking '{}' phase as manually completed...", phase_name),
                        None,
                    ).await?;
                    let bot = self.clone();
                    tokio::spawn(async move {
                        match runner.mark_phase_done(&ritual_id).await {
                            Ok(new_state) => {
                                let msg = format!(
                                    "✅ '{}' done. Now in: {} phase.",
                                    phase_name, new_state.phase.display_name()
                                );
                                let _ = bot.send_message(chat_id, &msg, None).await;
                            }
                            Err(e) => {
                                let _ = bot.send_message(chat_id, &format!("❌ Mark done failed: {}", e), None).await;
                            }
                        }
                    });
                }
            }
            arg if arg.starts_with("resume-from ") || arg.starts_with("resume ") => {
                let rest = arg.strip_prefix("resume-from ")
                    .or_else(|| arg.strip_prefix("resume "))
                    .unwrap_or("")
                    .trim();

                // First word is phase name, rest (if any) is task description
                let (phase_str, inline_task) = match rest.split_once(' ') {
                    Some((p, t)) => (p.trim(), Some(t.trim().to_string())),
                    None => (rest, None),
                };
                
                let phase = match crate::ritual_runner::parse_phase_name(phase_str) {
                    Some(p) => p,
                    None => {
                        self.send_message(
                            chat_id,
                            &format!(
                                "❌ Unknown phase: '{}'\n\nValid phases: design, review, plan, graph, implement, verify, triage, requirements",
                                phase_str
                            ),
                            None,
                        ).await?;
                        return Ok(());
                    }
                };

                if phase.is_terminal() {
                    self.send_message(chat_id, "❌ Cannot resume from a terminal phase (done/cancelled/escalated).", None).await?;
                    return Ok(());
                }

                // Task source priority: inline task > existing ritual task
                let runner = self.make_ritual_runner(chat_id);
                let existing_task = runner.load_state().ok()
                    .filter(|s| !s.task.is_empty())
                    .map(|s| s.task.clone());

                let task = inline_task.or(existing_task);
                let task = match task {
                    Some(t) => t,
                    None => {
                        self.send_message(
                            chat_id,
                            "⚠️ No task description found. Usage: `/ritual resume-from <phase> <task description>`",
                            None,
                        ).await?;
                        return Ok(());
                    }
                };

                let phase_display = phase.display_name().to_string();
                self.send_message(
                    chat_id,
                    &format!("🔄 Resuming ritual from {} phase...", phase_display),
                    None,
                ).await?;

                let bot = self.clone();
                let notify_fn = self.make_notify_fn(chat_id);
                let cancel_registry = self.ritual_cancel_registry.clone();
                let event_registry = self.ritual_event_registry.clone();
                tokio::spawn(async move {
                    let ritual_llm = match crate::llm::create_client(&bot.runner.config().llm) {
                        Ok(c) => Arc::new(tokio::sync::RwLock::new(c)),
                        Err(e) => {
                            let _ = bot.send_message(chat_id, &format!("❌ Failed to create LLM client: {}", e), None).await;
                            return;
                        }
                    };
                    let runner = crate::ritual_runner::RitualRunner::with_registries(
                        bot.runner.workspace_root().to_path_buf(),
                        ritual_llm,
                        notify_fn,
                        cancel_registry,
                        event_registry,
                    ).with_agent_runner(bot.runner.clone());

                    match runner.resume_from_phase(task, phase, None).await {
                        Ok(state) => {
                            if let Err(e) = runner.save_state(&state) {
                                tracing::error!("Failed to save ritual state: {}", e);
                            }
                            tracing::info!("Resume-from completed in {} phase", state.phase.display_name());
                        }
                        Err(e) => {
                            let _ = bot.send_message(chat_id, &format!("❌ Resume failed: {}", e), None).await;
                        }
                    }
                });
            }
            "" => {
                self.send_message(
                    chat_id,
                    "🔧 **Ritual Commands**\n\n\
                     `/ritual <task>` — Start a new development ritual\n\
                     `/ritual status` — Show current ritual status\n\
                     `/ritual cancel [id]` — Cancel a ritual (latest or by ID)\n\
                     `/ritual retry` — Retry from escalated state\n\
                     `/ritual skip` — Skip current phase\n\
                     `/ritual done` — Mark current phase as manually completed\n\
                     `/ritual resume-from <phase>` — Resume from a specific phase (design, review, plan, graph, implement, verify)\n\
                     `/ritual approve [findings]` — Approve review findings (e.g., `approve FINDING-1,3` or `approve all`)\n\
                     `/ritual clarify <response>` — Answer clarification question",
                    None,
                ).await?;
            }
            arg if arg.starts_with("clarify ") => {
                let clarification = arg.strip_prefix("clarify ").unwrap_or("").trim().to_string();
                if clarification.is_empty() {
                    self.send_message(chat_id, "⚠️ Usage: `/ritual clarify <your response>`", None).await?;
                } else {
                    let runner = self.make_ritual_runner(chat_id);
                    let state = runner.load_state()?;
                    if state.phase != gid_core::ritual::V2Phase::WaitingClarification {
                        self.send_message(
                            chat_id,
                            "⚠️ Clarify is only available when ritual is waiting for clarification.",
                            None,
                        ).await?;
                    } else {
                        self.send_message(chat_id, "💬 Received clarification, re-triaging...", None).await?;
                        let bot = self.clone();
                        tokio::spawn(async move {
                            match runner.send_event(RitualEvent::UserClarification { response: clarification }).await {
                                Ok(state) => {
                                    if let Err(e) = runner.save_state(&state) {
                                        tracing::error!("Failed to save ritual state: {}", e);
                                    }
                                    tracing::info!("Clarification processed, now in {} phase", state.phase.display_name());
                                }
                                Err(e) => {
                                    let _ = bot.send_message(chat_id, &format!("❌ Clarification failed: {}", e), None).await;
                                }
                            }
                        });
                    }
                }
            }
            arg if arg.starts_with("approve ") || arg == "approve"
                || arg.starts_with("apply ") || arg == "apply" => {
                // "apply" is a common alias for "approve" (users say "apply all" for review findings)
                let approved = arg.strip_prefix("approve ")
                    .or_else(|| arg.strip_prefix("apply "))
                    .unwrap_or("all")
                    .trim()
                    .to_string();
                let runner = self.make_ritual_runner(chat_id);
                // Find rituals waiting for approval
                let waiting: Vec<_> = runner.list_rituals()?
                    .into_iter()
                    .filter(|r| r.phase == gid_core::ritual::V2Phase::WaitingApproval)
                    .collect();

                let state = match waiting.len() {
                    0 => {
                        self.send_message(
                            chat_id,
                            "⚠️ No ritual is waiting for approval. Use `/ritual status` to check.",
                            None,
                        ).await?;
                        return Ok(());
                    }
                    1 => waiting.into_iter().next().unwrap(),
                    _ => {
                        // Multiple waiting — ask user which one
                        let mut msg = "⚠️ Multiple rituals waiting for approval:\n".to_string();
                        for (i, r) in waiting.iter().enumerate() {
                            let task_preview: String = r.task.chars().take(60).collect();
                            msg.push_str(&format!("{}. `{}` — {}\n", i + 1, r.id, task_preview));
                        }
                        msg.push_str("\nSpecify which: `/ritual approve all` applies to the most recent.\nOr cancel unwanted rituals with `/ritual cancel <id>`.");
                        self.send_message(chat_id, &msg, None).await?;
                        // Default: use most recent (already sorted by updated_at desc)
                        waiting.into_iter().next().unwrap()
                    }
                };

                if state.phase != gid_core::ritual::V2Phase::WaitingApproval {
                    self.send_message(
                        chat_id,
                        "⚠️ Approve is only available when ritual is waiting for review approval.",
                        None,
                    ).await?;
                } else {
                    let ritual_id = state.id.clone();
                    let task_preview: String = state.task.chars().take(60).collect();
                    self.send_message(chat_id, &format!("✅ Applying findings to ritual '{}': {}", task_preview, approved), None).await?;
                    let bot = self.clone();
                    tokio::spawn(async move {
                        match runner.send_event_to(&ritual_id, RitualEvent::UserApproval { approved }).await {
                            Ok(state) => {
                                if let Err(e) = runner.save_state(&state) {
                                    tracing::error!("Failed to save ritual state: {}", e);
                                }
                                tracing::info!("Approval processed, now in {} phase", state.phase.display_name());
                            }
                            Err(e) => {
                                let _ = bot.send_message(chat_id, &format!("❌ Approval failed: {}", e), None).await;
                            }
                        }
                    });
                }
            }
            task => {
                // Guard: reject short/ambiguous text that looks like a mistyped command
                // rather than a real task description. This prevents "/ritual apply all"
                // from starting a ritual with task "apply all".
                let normalized = task.trim().to_lowercase();
                let looks_like_command = matches!(
                    normalized.as_str(),
                    "apply" | "apply all" | "approve" | "approve all"
                    | "yes" | "no" | "ok" | "skip" | "retry" | "cancel"
                    | "help" | "list" | "status"
                ) || normalized.len() < 10;

                if looks_like_command {
                    // Check if there's a waiting ritual that should receive this
                    let runner = self.make_ritual_runner(chat_id);
                    if let Ok(state) = runner.load_state() {
                        if state.phase == gid_core::ritual::V2Phase::WaitingApproval
                            || state.phase == gid_core::ritual::V2Phase::WaitingClarification
                        {
                            // Route to the waiting ritual instead of starting a new one
                            if let Some(event) = self.build_ritual_event_from_text(task, &state) {
                                self.send_message(chat_id, "💬 Routing to waiting ritual...", None).await?;
                                let bot = self.clone();
                                tokio::spawn(async move {
                                    match runner.send_event(event).await {
                                        Ok(new_state) => {
                                            if let Err(e) = runner.save_state(&new_state) {
                                                tracing::error!("Failed to save ritual state: {}", e);
                                            }
                                        }
                                        Err(e) => {
                                            let _ = bot.send_message(chat_id, &format!("❌ {}", e), None).await;
                                        }
                                    }
                                });
                                return Ok(());
                            }
                        }
                    }
                    // No waiting ritual — tell user this doesn't look like a task
                    self.send_message(
                        chat_id,
                        &format!("⚠️ '{}' doesn't look like a task description. Did you mean:\n\
                            • `/ritual approve all` — approve review findings\n\
                            • `/ritual status` — check ritual status\n\
                            • `/ritual cancel` — cancel current ritual\n\n\
                            To start a new ritual, provide a full task description.",
                            task
                        ),
                        None,
                    ).await?;
                    return Ok(());
                }

                // Start new ritual — check if project path can be auto-detected
                // If not, show project selector inline keyboard
                let has_explicit_project = crate::ritual_runner::has_target_project_dir(task);
                
                if has_explicit_project {
                    // Project path found in task text — start immediately
                    self.send_message(chat_id, &format!("🚀 Starting ritual: \"{}\"", task), None).await?;
                    self.spawn_ritual(chat_id, task.to_string(), self.runner.workspace_root().to_path_buf());
                } else {
                    // No project path — show project selector
                    let projects = self.discover_projects();
                    if projects.is_empty() {
                        // No known projects, start with workspace root
                        self.send_message(chat_id, &format!("🚀 Starting ritual: \"{}\"", task), None).await?;
                        self.spawn_ritual(chat_id, task.to_string(), self.runner.workspace_root().to_path_buf());
                    } else if projects.len() == 1 {
                        // Only one project — use it directly
                        let project_path = std::path::PathBuf::from(&projects[0].1);
                        self.send_message(chat_id, &format!("🚀 Starting ritual in `{}`: \"{}\"", projects[0].0, task), None).await?;
                        self.spawn_ritual(chat_id, task.to_string(), project_path);
                    } else {
                        // Multiple projects — show inline keyboard
                        let mut pending = self.pending_ritual_tasks.lock().await;
                        pending.insert(chat_id, task.to_string());
                        
                        let mut buttons = Vec::new();
                        for (name, path) in &projects {
                            buttons.push(serde_json::json!([{
                                "text": format!("📁 {}", name),
                                "callback_data": format!("__ritual_project:{}", path)
                            }]));
                        }
                        
                        let payload = serde_json::json!({
                            "chat_id": chat_id,
                            "text": format!("📂 Select project for ritual:\n\"{}\"", task),
                            "reply_markup": { "inline_keyboard": buttons }
                        });
                        
                        self.client
                            .post(self.api_url("sendMessage"))
                            .json(&payload)
                            .send()
                            .await?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Route a user message to a waiting ritual if one exists.
    /// Returns true if the message was routed, false if no ritual is waiting.
    ///
    /// Checks disk for rituals in WaitingApproval/WaitingClarification state.
    /// Calls advance() which is stateless — no channels, no loops, just load → transition → save.
    async fn try_route_to_waiting_ritual(&self, chat_id: i64, text: &str) -> bool {
        let runner = self.make_ritual_runner(chat_id);
        let state = match runner.load_state() {
            Ok(s) => s,
            Err(_) => return false,
        };

        // Only intercept if a ritual is actively waiting for user input
        if !state.phase.is_paused() {
            return false;
        }

        let event = match self.build_ritual_event_from_text(text, &state) {
            Some(e) => e,
            None => return false, // Doesn't look like a ritual response → let main agent handle
        };

        let ritual_id = state.id.clone();
        let bot = self.clone();
        tokio::spawn(async move {
            match runner.send_event_to(&ritual_id, event).await {
                Ok(_) => {
                    tracing::info!(ritual_id = %ritual_id, "User message routed to waiting ritual via advance");
                }
                Err(e) => {
                    let _ = bot.send_message(chat_id, &format!("❌ Failed to resume ritual: {}", e), None).await;
                }
            }
        });
        true
    }

    /// Create a notify callback that sends Telegram messages to the given chat.
    fn make_ritual_runner(&self, chat_id: i64) -> crate::ritual_runner::RitualRunner {
        crate::ritual_runner::RitualRunner::with_registries(
            self.runner.workspace_root().to_path_buf(),
            self.runner.llm_client(),
            self.make_notify_fn(chat_id),
            self.ritual_cancel_registry.clone(),
            self.ritual_event_registry.clone(),
        ).with_agent_runner(self.runner.clone())
    }

    /// Discover known projects that have `.gid/` directories (indicating GID-managed projects).
    /// Returns (display_name, absolute_path) pairs.
    fn discover_projects(&self) -> Vec<(String, String)> {
        let mut projects = Vec::new();
        
        // Check known project directories
        let search_dirs = [
            self.runner.workspace_root().to_path_buf(), // RustClaw itself
        ];
        
        // Also check clawd/projects/ if it exists
        let clawd_projects = std::path::PathBuf::from("/Users/potato/clawd/projects");
        
        // Add the workspace root if it has .gid/
        for dir in &search_dirs {
            if dir.join(".gid").is_dir() {
                let name = dir.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| dir.to_string_lossy().to_string());
                projects.push((name, dir.to_string_lossy().to_string()));
            }
        }
        
        // Scan clawd/projects/ for sub-projects
        if clawd_projects.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&clawd_projects) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() && path.join(".gid").is_dir() {
                        let name = path.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let abs = path.to_string_lossy().to_string();
                        // Avoid duplicates
                        if !projects.iter().any(|(_, p)| p == &abs) {
                            projects.push((name, abs));
                        }
                    }
                }
            }
        }
        
        projects.sort_by(|a, b| a.0.cmp(&b.0));
        projects
    }

    /// Start a ritual with a specific project root, spawning it in a background task.
    fn spawn_ritual(&self, chat_id: i64, task: String, project_root: std::path::PathBuf) {
        let bot = self.clone();
        let notify_fn = self.make_notify_fn(chat_id);
        let cancel_registry = self.ritual_cancel_registry.clone();
        let event_registry = self.ritual_event_registry.clone();
        tokio::spawn(async move {
            let ritual_llm = match crate::llm::create_client(&bot.runner.config().llm) {
                Ok(c) => Arc::new(tokio::sync::RwLock::new(c)),
                Err(e) => {
                    let _ = bot.send_message(chat_id, &format!("❌ Failed to create ritual LLM client: {}", e), None).await;
                    return;
                }
            };
            let runner = crate::ritual_runner::RitualRunner::with_registries(
                project_root,
                ritual_llm,
                notify_fn,
                cancel_registry,
                event_registry,
            ).with_agent_runner(bot.runner.clone());
            match runner.start(task).await {
                Ok(state) => {
                    if let Err(e) = runner.save_state(&state) {
                        tracing::error!("Failed to save final ritual state: {}", e);
                    }
                    tracing::info!("Ritual finished in {} phase", state.phase.display_name());
                }
                Err(e) => {
                    let _ = bot.send_message(
                        chat_id,
                        &format!("❌ Ritual failed: {}", e),
                        None,
                    ).await;
                }
            }
        });
    }

    /// Build a ritual event from user free-text based on the ritual's current phase.
    /// Returns None if the text doesn't look like a ritual response — in that case
    /// the message should go to the main agent instead.
    ///
    /// For WaitingApproval: only matches explicit approval/skip patterns.
    /// For WaitingClarification: any text is treated as clarification.
    fn build_ritual_event_from_text(
        &self,
        text: &str,
        state: &gid_core::ritual::V2State,
    ) -> Option<gid_core::ritual::V2Event> {
        use gid_core::ritual::V2Phase;
        use gid_core::ritual::V2Event;

        let normalized = text.trim().to_lowercase();

        if state.phase == V2Phase::WaitingApproval {
            // Only match explicit approval patterns — don't hijack unrelated messages
            if normalized == "skip" || normalized == "跳过" {
                return Some(V2Event::UserSkipPhase);
            }
            // "apply all", "apply 1,3", "approve all", "all", "yes", "ok", "好"
            if normalized.starts_with("apply ")
                || normalized.starts_with("approve ")
                || normalized == "apply" || normalized == "approve"
                || normalized == "all" || normalized == "apply all" || normalized == "approve all"
                || normalized == "yes" || normalized == "ok" || normalized == "好"
                || normalized == "全部应用" || normalized == "应用"
            {
                let approved = normalized
                    .strip_prefix("apply ")
                    .or_else(|| normalized.strip_prefix("approve "))
                    .unwrap_or("all")
                    .trim()
                    .to_string();
                return Some(V2Event::UserApproval { approved });
            }
            // Numbered selection like "1,3,5" or "FINDING-1,3"
            if normalized.contains("finding") || normalized.chars().all(|c| c.is_ascii_digit() || c == ',' || c == ' ') {
                return Some(V2Event::UserApproval { approved: normalized });
            }
            // Doesn't look like an approval — let it go to main agent
            None
        } else if state.phase == V2Phase::WaitingClarification {
            // Any text is valid clarification
            Some(V2Event::UserClarification { response: text.to_string() })
        } else {
            None
        }
    }

    /// Handle a sub-agent lifecycle event: trigger a proactive agent turn so the agent
    /// knows its sub-agent completed/failed and can act on it.
    async fn handle_subagent_event(&self, event: crate::events::SubAgentEvent) {
        let (parent_key, system_msg) = match &event {
            crate::events::SubAgentEvent::Completed { task_id, parent_session_key, task_summary, result_preview, files_modified, duration_secs } => {
                let files_str = if files_modified.is_empty() {
                    "(none)".to_string()
                } else {
                    files_modified.join(", ")
                };
                let msg = format!(
                    "[system] Your sub-agent '{}' has completed ({:.0}s).\nTask: {}\nFiles modified: {}\nResult summary: {}",
                    task_id, duration_secs, task_summary, files_str, result_preview
                );
                (parent_session_key.clone(), msg)
            }
            crate::events::SubAgentEvent::Failed { task_id, parent_session_key, task_summary, error, files_modified, duration_secs } => {
                let files_str = if files_modified.is_empty() {
                    "(none)".to_string()
                } else {
                    files_modified.join(", ")
                };
                let msg = format!(
                    "[system] Your sub-agent '{}' has FAILED ({:.0}s).\nTask: {}\nError: {}\nFiles modified before failure: {}",
                    task_id, duration_secs, task_summary, error, files_str
                );
                (parent_session_key.clone(), msg)
            }
        };

        if parent_key.is_empty() {
            tracing::warn!("Sub-agent event with empty parent session key, skipping proactive turn");
            return;
        }

        // Extract chat_id from parent session key (format: "telegram:{chat_id}")
        let chat_id = match parent_key.strip_prefix("telegram:").and_then(|s| s.parse::<i64>().ok()) {
            Some(id) => id,
            None => {
                tracing::warn!("Cannot parse chat_id from parent session key: {}", parent_key);
                return;
            }
        };

        tracing::info!("Triggering proactive agent turn for sub-agent event → {}", parent_key);

        // Process the system message through the agent and stream response to Telegram
        let mut rx = self.runner.process_message_events(
            &parent_key,
            &system_msg,
            None, // system message, no user
            Some("telegram"),
            false,
        );

        // Consume events and send responses to Telegram
        while let Some(event) = rx.recv().await {
            match event {
                crate::events::AgentEvent::Response(text) => {
                    if !text.is_empty() && text != "HEARTBEAT_OK" {
                        if let Err(e) = self.send_message(chat_id, &text, None).await {
                            tracing::error!("Failed to send proactive response: {}", e);
                        }
                    }
                }
                crate::events::AgentEvent::Error(e) => {
                    tracing::error!("Error in proactive agent turn: {}", e);
                }
                _ => {} // Ignore intermediate events
            }
        }
    }

    fn make_notify_fn(&self, chat_id: i64) -> crate::ritual_runner::NotifyFn {
        let bot = self.clone();
        Arc::new(move |msg: String| {
            let bot = bot.clone();
            Box::pin(async move {
                if let Err(e) = bot.send_message(chat_id, &msg, None).await {
                    tracing::error!("Failed to send ritual notification: {}", e);
                }
            })
        })
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
                        "text": format!("✅ Model: <code>{}</code>", model_id),
                        "parse_mode": "HTML",
                        "reply_markup": { "inline_keyboard": buttons }
                    }))
                    .send()
                    .await;
            }
            return Ok(());
        }

        // Handle ritual project selection callbacks
        if let Some(project_path) = data.strip_prefix("__ritual_project:") {
            // Retrieve the pending ritual task for this chat
            let task = {
                let mut pending = self.pending_ritual_tasks.lock().await;
                pending.remove(&chat_id)
            };
            
            if let Some(task) = task {
                let project_name = std::path::Path::new(project_path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| project_path.to_string());
                self.answer_callback_query(callback_id, Some(&format!("Selected: {}", project_name))).await?;
                
                // Update the message to show selection
                if let Some(msg_id) = message_id {
                    let _ = self.client
                        .post(self.api_url("editMessageText"))
                        .json(&serde_json::json!({
                            "chat_id": chat_id,
                            "message_id": msg_id,
                            "text": format!("🚀 Starting ritual in `{}`:\n\"{}\"", project_name, task),
                        }))
                        .send()
                        .await;
                }
                
                self.spawn_ritual(chat_id, task, std::path::PathBuf::from(project_path));
            } else {
                self.answer_callback_query(callback_id, Some("⚠️ No pending ritual task")).await?;
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
    // detect_voice_mode_toggle removed — voice mode toggle is now 100% LLM-driven
    // via set_voice_mode tool. No hardcoded patterns = no mismatches.

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

        // Save to temp file with unique name to avoid race conditions
        let ogg_path = format!("/tmp/rustclaw_voice_{}.ogg", uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or("tmp"));
        tokio::fs::write(&ogg_path, &file_bytes).await?;
        tracing::debug!("Downloaded voice to {}", ogg_path);

        // Step 3: Transcribe using STT
        let transcription = stt::transcribe(&ogg_path).await?;
        tracing::info!("Transcribed: {}", { let _end = transcription.len().min(50); let _end = transcription.floor_char_boundary(_end); &transcription[.._end] });

        // Clean up the input file
        let _ = tokio::fs::remove_file(ogg_path).await;

        // Voice mode toggle is handled by LLM via set_voice_mode tool.

        // Step 4: Process transcribed text as a normal message.
        // No prefix — agent treats it the same as typed text.
        let user_message = transcription;
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
        let html_text = crate::markdown::to_telegram_html(text);
        let mut payload = serde_json::json!({
            "chat_id": chat_id,
            "text": html_text,
            "parse_mode": "HTML",
        });
        let resp = self.client
            .post(self.api_url("sendMessage"))
            .json(&payload)
            .send()
            .await?;
        
        // Fallback to plain text on HTML parse failure
        let response = if resp.status() == 400 {
            tracing::warn!("HTML parse failed in send_message_get_id, falling back to plain text");
            payload["text"] = serde_json::json!(crate::markdown::strip_markdown(text));
            payload.as_object_mut().unwrap().remove("parse_mode");
            self.client
                .post(self.api_url("sendMessage"))
                .json(&payload)
                .send()
                .await?
                .json::<serde_json::Value>()
                .await?
        } else {
            resp.json::<serde_json::Value>().await?
        };

        let message_id = response["result"]["message_id"]
            .as_i64()
            .ok_or_else(|| anyhow::anyhow!("No message_id in response"))?;

        Ok(message_id)
    }

    /// Edit an existing message.
    async fn edit_message(&self, chat_id: i64, message_id: i64, text: &str) -> anyhow::Result<()> {
        let html_text = crate::markdown::to_telegram_html(text);
        let mut payload = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": html_text,
            "parse_mode": "HTML",
        });
        let response = self.client
            .post(self.api_url("editMessageText"))
            .json(&payload)
            .send()
            .await?;

        if response.status().as_u16() == 400 {
            // HTML parse failed, fall back to stripped plain text
            tracing::warn!("HTML parse failed in edit_message, falling back to plain text");
            payload["text"] = serde_json::json!(crate::markdown::strip_markdown(text));
            payload.as_object_mut().unwrap().remove("parse_mode");
            let retry = self.client
                .post(self.api_url("editMessageText"))
                .json(&payload)
                .send()
                .await?;
            if !retry.status().is_success() {
                let body = retry.text().await.unwrap_or_default();
                tracing::debug!("Edit message plain text also failed: {}", body);
            }
        } else if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            tracing::debug!("Edit message failed ({}): {}", status, body);
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
        let mut consecutive_errors: u32 = 0;
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
                    consecutive_errors = 0;
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
                            // Spawn each update handler concurrently so /stop can
                            // execute while a long-running agent call is in progress.
                            let this = self.clone();
                            let update = update.clone();
                            tokio::spawn(async move {
                                if let Err(e) = this.handle_update(&update).await {
                                    tracing::error!("Update handling error: {}", e);
                                }
                            });
                        }
                    }
                }
                Err(e) => {
                    consecutive_errors += 1;
                    tracing::warn!("Polling error (#{consecutive_errors}): {e}. Retrying in 5s...");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    }
}

/// Format token count with K/M suffix for readability.
fn format_token_count(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        format!("{}", tokens)
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

    // Spawn sub-agent event listener for proactive completion handling
    let bot_clone = bot.clone();
    let mut event_rx = bot.runner.subagent_events.subscribe();
    tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok(event) => {
                    bot_clone.handle_subagent_event(event).await;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("Sub-agent event listener lagged, missed {} events", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    tracing::info!("Sub-agent event channel closed, stopping listener");
                    break;
                }
            }
        }
    });

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
