//! Discord channel adapter using serenity.
//!
//! Connects via Discord Gateway, handles messages, mentions, reactions,
//! and file attachments. Supports structured MessageContext for rich
//! sender metadata and reply-to propagation.

use std::sync::Arc;

use serenity::all::{
    ChannelId, Context, CreateAttachment, CreateMessage, EventHandler, GatewayIntents, Message,
    Ready,
};
use serenity::async_trait;
use serenity::Client;

use crate::agent::AgentRunner;
use crate::config::DiscordConfig;
use crate::context::{self, ChannelCapabilities, ChatType, MessageContext, QuotedMessage};
use crate::text_utils;

/// Discord bot handler.
struct DiscordHandler {
    config: DiscordConfig,
    runner: Arc<AgentRunner>,
    /// Bot user ID (fetched on Ready)
    bot_id: std::sync::RwLock<Option<serenity::all::UserId>>,
}

impl DiscordHandler {
    fn new(config: DiscordConfig, runner: Arc<AgentRunner>) -> Self {
        Self {
            config,
            runner,
            bot_id: std::sync::RwLock::new(None),
        }
    }

    /// Return Discord channel capabilities.
    fn capabilities() -> ChannelCapabilities {
        ChannelCapabilities {
            name: "discord".into(),
            supports_reactions: true,
            supports_inline_buttons: false,
            supports_voice: false,
            supports_reply_to: true,
            supports_typing: true,
            supports_markdown: true,
            supports_tables: false,
            max_message_length: 2000,
            format_notes: vec![
                "Use bullet lists instead of tables — Discord renders them poorly on mobile".into(),
                "Code blocks use triple backticks with optional language hint".into(),
                "Bold: **text**, Italic: *text*, Strikethrough: ~~text~~".into(),
                "Spoiler: ||text||, Links: [text](url)".into(),
            ],
        }
    }

    /// Check if a message should be processed based on guild/channel filters.
    fn should_process(&self, msg: &Message) -> bool {
        // Ignore bot messages
        if msg.author.bot {
            return false;
        }

        // Check guild filter
        if let Some(guild_id) = msg.guild_id {
            if !self.config.allowed_guilds.is_empty()
                && !self.config.allowed_guilds.contains(&guild_id.get())
            {
                return false;
            }
        }

        // Check channel filter
        if !self.config.allowed_channels.is_empty()
            && !self.config.allowed_channels.contains(&msg.channel_id.get())
        {
            return false;
        }

        true
    }

    /// Check if bot is mentioned in the message.
    fn is_mentioned(&self, msg: &Message) -> bool {
        let bot_id = self.bot_id.read().unwrap();
        if let Some(id) = *bot_id {
            msg.mentions.iter().any(|u| u.id == id)
        } else {
            false
        }
    }

    /// Strip bot mention from message content.
    fn strip_mention(&self, content: &str) -> String {
        let bot_id = self.bot_id.read().unwrap();
        if let Some(id) = *bot_id {
            // Discord mentions are formatted as <@USER_ID> or <@!USER_ID>
            let mention1 = format!("<@{}>", id);
            let mention2 = format!("<@!{}>", id);
            content
                .replace(&mention1, "")
                .replace(&mention2, "")
                .trim()
                .to_string()
        } else {
            content.to_string()
        }
    }

    /// Convert Telegram-style MarkdownV2 to Discord markdown.
    fn convert_markdown(text: &str) -> String {
        // Discord uses similar markdown but with some differences:
        // - Bold: **text** (same)
        // - Italic: *text* or _text_ (same)
        // - Code: `code` (same)
        // - Strikethrough: ~~text~~ (same)
        // - Spoiler: ||text|| (Discord-specific)
        // - Links: [text](url) (same)

        // Remove Telegram-specific escaping (backslashes before special chars)
        let mut result = text.to_string();

        // Unescape characters that Telegram MarkdownV2 escapes but Discord doesn't need
        let unescape_chars = [
            '_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.',
            '!',
        ];
        for c in unescape_chars {
            result = result.replace(&format!("\\{}", c), &c.to_string());
        }

        result
    }

    /// Build a MessageContext from a Discord message.
    fn build_message_context(&self, msg: &Message) -> MessageContext {
        let chat_type = if msg.guild_id.is_some() {
            // Guild message — try to get guild name
            ChatType::Group {
                title: msg
                    .guild_id
                    .map(|gid| format!("guild:{}", gid)),
            }
        } else {
            ChatType::Direct
        };

        // Build reply-to context if this is a reply
        let reply_to = msg.referenced_message.as_ref().map(|ref_msg| {
            QuotedMessage::from_discord(
                &ref_msg.author.name,
                &ref_msg.author.id.to_string(),
                &ref_msg.content,
                ref_msg.id.get(),
            )
        });

        MessageContext {
            sender_id: Some(msg.author.id.to_string()),
            sender_name: Some(msg.author.name.clone()),
            sender_username: msg.author.global_name.clone(),
            chat_type,
            reply_to,
            message_id: Some(msg.id.get() as i64),
        }
    }

    /// Send a message, handling long messages and file attachments.
    /// Uses ProcessedResponse to handle reply_to tags.
    async fn send_response(
        &self,
        ctx: &Context,
        channel_id: ChannelId,
        response: &str,
        _original_msg_id: Option<u64>,
    ) -> anyhow::Result<()> {
        let processed = context::ProcessedResponse::from_raw(response);

        if processed.is_silent {
            return Ok(());
        }

        let converted = Self::convert_markdown(&processed.text);

        // Check for FILE: patterns
        let file_re = regex::Regex::new(r"FILE:(/[^\s]+)").unwrap();
        let mut text_without_files = converted.clone();
        let mut files_to_send: Vec<String> = Vec::new();

        for cap in file_re.captures_iter(&converted) {
            let file_path = cap[1].to_string();
            files_to_send.push(file_path.clone());
            text_without_files =
                text_without_files.replace(&format!("FILE:{}", file_path), "");
        }

        let clean_text = text_without_files.trim();

        // Discord message limit is 2000 chars
        if !clean_text.is_empty() {
            let chunks = text_utils::split_message(clean_text, 2000);
            for (i, chunk) in chunks.iter().enumerate() {
                let mut builder = CreateMessage::new().content(chunk.to_string());
                // Reply to the original message for the first chunk if reply_to is set
                if i == 0 {
                    if let Some(reply_id) = processed.reply_to {
                        builder = builder.reference_message(serenity::all::MessageReference::from(
                            (channel_id, serenity::all::MessageId::new(reply_id as u64)),
                        ));
                    }
                }
                channel_id.send_message(&ctx.http, builder).await?;
            }
        }

        // Send files
        for file_path in files_to_send {
            if std::path::Path::new(&file_path).exists() {
                let attachment = CreateAttachment::path(&file_path).await?;
                let builder = CreateMessage::new().add_file(attachment);
                channel_id.send_message(&ctx.http, builder).await?;
            } else {
                let builder =
                    CreateMessage::new().content(format!("⚠️ File not found: {}", file_path));
                channel_id.send_message(&ctx.http, builder).await?;
            }
        }

        Ok(())
    }
}

#[async_trait]
impl EventHandler for DiscordHandler {
    async fn ready(&self, _ctx: Context, ready: Ready) {
        let discriminator = ready.user.discriminator.map(|d| d.get()).unwrap_or(0);
        tracing::info!(
            "Discord bot connected as {}#{}",
            ready.user.name,
            discriminator
        );
        *self.bot_id.write().unwrap() = Some(ready.user.id);

        // Set channel capabilities on the runner
        self.runner
            .set_channel_capabilities(Self::capabilities())
            .await;
    }

    async fn message(&self, ctx: Context, msg: Message) {
        // Check if we should process this message
        if !self.should_process(&msg) {
            return;
        }

        let is_dm = msg.guild_id.is_none();
        let is_mentioned = self.is_mentioned(&msg);

        // Apply group policy for non-DM messages
        if !is_dm {
            match self.config.group_policy.as_str() {
                "off" => {
                    tracing::debug!("Ignoring guild message (policy: off)");
                    return;
                }
                "mention" => {
                    if !is_mentioned {
                        tracing::debug!("Ignoring guild message (no mention)");
                        return;
                    }
                }
                "open" => {
                    // Respond to all messages
                }
                _ => {
                    // Default to mention-only
                    if !is_mentioned {
                        return;
                    }
                }
            }
        }

        // Strip mention from content
        let content = if is_mentioned {
            self.strip_mention(&msg.content)
        } else {
            msg.content.clone()
        };

        if content.is_empty() {
            // Just a mention with no text
            if let Err(e) = msg.channel_id.say(&ctx.http, "Yes?").await {
                tracing::error!("Failed to send response: {}", e);
            }
            return;
        }

        // Build session key
        let session_key = format!(
            "discord:{}:{}",
            msg.guild_id
                .map(|g| g.to_string())
                .unwrap_or_else(|| "dm".to_string()),
            msg.channel_id
        );

        // Build structured message context
        let msg_ctx = self.build_message_context(&msg);

        tracing::info!(
            "Discord message from {} in {}: {}",
            msg.author.name,
            session_key,
            text_utils::truncate_chars(&content, 50)
        );

        // Show typing indicator
        let typing = msg.channel_id.start_typing(&ctx.http);

        // Process with agent using structured context
        match self
            .runner
            .process_message_with_context(&session_key, &content, &msg_ctx, false)
            .await
        {
            Ok(response) => {
                drop(typing); // Stop typing indicator
                if !response.is_silent {
                    if let Err(e) = self
                        .send_response(&ctx, msg.channel_id, &response.text, Some(msg.id.get()))
                        .await
                    {
                        tracing::error!("Failed to send response: {}", e);
                    }
                }
            }
            Err(e) => {
                drop(typing);
                tracing::error!("Agent error: {}", e);
                if let Err(send_err) = msg
                    .channel_id
                    .say(&ctx.http, format!("⚠️ Error: {}", e))
                    .await
                {
                    tracing::error!("Failed to send error message: {}", send_err);
                }
            }
        }
    }
}

/// Start the Discord channel.
pub async fn start(config: DiscordConfig, runner: Arc<AgentRunner>) -> anyhow::Result<()> {
    let handler = DiscordHandler::new(config.clone(), runner);

    // Configure intents
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    let mut client = Client::builder(&config.bot_token, intents)
        .event_handler(handler)
        .await?;

    tracing::info!("Starting Discord client...");
    client.start().await?;

    Ok(())
}
