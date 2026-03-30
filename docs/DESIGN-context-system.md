# Design: Context System Refactor

## Problem

RustClaw's system prompt is a hardcoded string in `workspace.rs`. Channel-specific context (sender info, reply-to messages, capabilities) doesn't flow through to the LLM. The `process_message` interface only passes `(session_key, user_message, user_id, channel)` as flat strings — no structured metadata.

This causes:
- LLM doesn't know who sent the message (name, not just ID)
- LLM can't see quoted/reply-to messages
- LLM doesn't know channel capabilities (reactions, inline buttons, voice)
- LLM doesn't know if it's a DM vs group chat
- No runtime context (OS, version, binary info)
- Response post-processing (VOICE:, NO_REPLY, reply tags) is scattered across telegram.rs
- Adding a new channel requires duplicating all this logic

## Design Goals

1. **Structured message metadata** flows from channel → agent → LLM context
2. **Channel declares its capabilities** so the LLM knows what it can do
3. **Response post-processing** is unified and channel-agnostic
4. **System prompt composition** is modular, not one giant string
5. **Backward compatible** — existing `process_message` still works, new API is additive

## Architecture

### New Types

```rust
// src/context.rs — new file

/// Per-message metadata from the channel
#[derive(Debug, Clone, Default)]
pub struct MessageContext {
    pub sender_id: Option<String>,
    pub sender_name: Option<String>,
    pub sender_username: Option<String>,
    pub chat_type: ChatType,
    pub reply_to: Option<QuotedMessage>,
    pub message_id: Option<i64>,
    pub timestamp: Option<DateTime<Local>>,
}

#[derive(Debug, Clone, Default)]
pub enum ChatType {
    #[default]
    Direct,
    Group { title: Option<String> },
}

#[derive(Debug, Clone)]
pub struct QuotedMessage {
    pub text: String,
    pub sender_name: Option<String>,
    pub message_id: Option<i64>,
}

/// Channel capability declaration
#[derive(Debug, Clone)]
pub struct ChannelCapabilities {
    pub name: String,                    // "telegram"
    pub supports_reactions: bool,        // emoji reactions
    pub supports_inline_buttons: bool,   // inline keyboard
    pub supports_voice: bool,            // voice messages
    pub supports_reply_to: bool,         // reply to specific message
    pub supports_typing: bool,           // typing indicator
    pub supports_markdown: bool,         // markdown formatting
    pub supports_tables: bool,           // markdown tables (Telegram: NO)
    pub max_message_length: usize,       // 4096 for Telegram
    pub format_notes: Vec<String>,       // ["No markdown tables", "Wrap code in ```"]
}

/// Runtime info injected once at startup
#[derive(Debug, Clone)]
pub struct RuntimeContext {
    pub os: String,                      // "Darwin 24.6.0 (arm64)"
    pub version: String,                 // "0.1.0"
    pub model: String,                   // "claude-opus-4-6"
    pub hostname: String,                // "potato's Mac mini"
    pub shell: String,                   // "zsh"
}

/// Processed LLM response with extracted control signals
#[derive(Debug)]
pub struct ProcessedResponse {
    pub text: String,
    pub reply_to: Option<i64>,           // [[reply_to:123]]
    pub voice_text: Option<String>,      // VOICE: prefix
    pub is_silent: bool,                 // NO_REPLY or HEARTBEAT_OK
    pub reaction: Option<String>,        // emoji to react with
}
```

### Channel Trait Extension

```rust
// src/channels/mod.rs

pub trait Channel: Send + Sync {
    fn name(&self) -> &str;
    
    /// Declare what this channel supports
    fn capabilities(&self) -> ChannelCapabilities;
    
    async fn start(&self, runner: Arc<AgentRunner>) -> anyhow::Result<()>;
}
```

Each channel implements `capabilities()`:

```rust
// telegram.rs
fn capabilities(&self) -> ChannelCapabilities {
    ChannelCapabilities {
        name: "telegram".into(),
        supports_reactions: true,
        supports_inline_buttons: true,
        supports_voice: true,
        supports_reply_to: true,
        supports_typing: true,
        supports_markdown: true,
        supports_tables: false,  // Telegram doesn't render MD tables
        max_message_length: 4096,
        format_notes: vec![
            "Use bullet lists instead of markdown tables".into(),
            "Code blocks use triple backticks".into(),
        ],
    }
}
```

### AgentRunner Interface

```rust
// src/agent.rs

/// New primary entry point
pub async fn process_message_with_context(
    &self,
    session_key: &str,
    user_message: &str,
    msg_ctx: &MessageContext,
    channel_caps: &ChannelCapabilities,
    is_heartbeat: bool,
) -> anyhow::Result<ProcessedResponse> {
    // 1. Build system prompt with full context
    // 2. Run agent loop
    // 3. Post-process response into ProcessedResponse
}

/// Backward compat — wraps process_message_with_context
pub async fn process_message(
    &self,
    session_key: &str,
    user_message: &str,
    user_id: Option<&str>,
    channel: Option<&str>,
) -> anyhow::Result<String> {
    let msg_ctx = MessageContext {
        sender_id: user_id.map(|s| s.to_string()),
        ..Default::default()
    };
    let caps = ChannelCapabilities::default(); // basic caps
    let response = self.process_message_with_context(
        session_key, user_message, &msg_ctx, &caps, false
    ).await?;
    Ok(response.text)
}
```

### System Prompt Builder

```rust
// src/workspace.rs — refactored

pub fn build_system_prompt_full(
    &self,
    runtime: &RuntimeContext,
    channel: &ChannelCapabilities,
    msg_ctx: Option<&MessageContext>,
    is_heartbeat: bool,
) -> String {
    let mut sections = Vec::new();
    
    // 1. Identity & Runtime
    sections.push(self.build_runtime_section(runtime));
    
    // 2. Behavioral Rules (tool style, safety, communication)
    sections.push(self.build_behavioral_rules());
    
    // 3. Channel Context & Formatting Rules
    sections.push(self.build_channel_section(channel));
    
    // 4. Voice instructions
    sections.push(self.build_voice_section());
    
    // 5. Memory recall rules
    sections.push(self.build_memory_rules());
    
    // 6. Workspace files (SOUL, AGENTS, USER, TOOLS, IDENTITY, MEMORY)
    sections.push(self.build_workspace_files());
    
    // 7. Skills
    sections.push(self.build_skills_section(is_heartbeat, msg_ctx));
    
    // 8. Per-message context (sender, quoted message)
    if let Some(ctx) = msg_ctx {
        sections.push(self.build_message_context(ctx));
    }
    
    // 9. Daily notes
    sections.push(self.build_daily_notes());
    
    sections.join("\n\n")
}

fn build_channel_section(&self, caps: &ChannelCapabilities) -> String {
    format!(
        "## Channel: {name}\n\
         Capabilities: {caps_list}\n\
         Max message length: {max_len}\n\
         {format_notes}",
        name = caps.name,
        caps_list = /* ... */,
        max_len = caps.max_message_length,
        format_notes = caps.format_notes.join("\n"),
    )
}

fn build_message_context(&self, ctx: &MessageContext) -> String {
    let mut s = "## Inbound Message Context\n".to_string();
    if let Some(name) = &ctx.sender_name {
        s.push_str(&format!("Sender: {} ({})\n", name, 
            ctx.sender_id.as_deref().unwrap_or("unknown")));
    }
    match &ctx.chat_type {
        ChatType::Direct => s.push_str("Chat type: Direct message\n"),
        ChatType::Group { title } => {
            s.push_str(&format!("Chat type: Group{}\n",
                title.as_ref().map(|t| format!(" ({})", t)).unwrap_or_default()));
        }
    }
    if let Some(reply) = &ctx.reply_to {
        s.push_str(&format!("\nReplied message:\n> {}\n— {}\n",
            reply.text,
            reply.sender_name.as_deref().unwrap_or("unknown")));
    }
    s
}
```

### Response Post-Processor

```rust
// src/context.rs

impl ProcessedResponse {
    /// Parse raw LLM output into structured response
    pub fn from_raw(raw: &str) -> Self {
        let trimmed = raw.trim();
        
        // Check silent
        let is_silent = trimmed == "NO_REPLY" || trimmed == "HEARTBEAT_OK";
        
        // Extract reply tag: [[reply_to:123]]
        let (text, reply_to) = Self::extract_reply_tag(trimmed);
        
        // Extract voice text: VOICE: prefix or \nVOICE: anywhere
        let voice_text = Self::extract_voice_text(&text);
        
        // Extract reaction
        let reaction = Self::extract_reaction(&text);
        
        ProcessedResponse {
            text: voice_text.as_ref().unwrap_or(&text).clone(),
            reply_to,
            voice_text,
            is_silent,
            reaction,
        }
    }
}
```

### Telegram Integration

```rust
// src/channels/telegram.rs — simplified

// In handle_update:
let msg_ctx = MessageContext {
    sender_id: Some(user_id.to_string()),
    sender_name: msg["from"]["first_name"].as_str().map(String::from),
    sender_username: msg["from"]["username"].as_str().map(String::from),
    chat_type: if is_group {
        ChatType::Group { title: msg["chat"]["title"].as_str().map(String::from) }
    } else {
        ChatType::Direct
    },
    reply_to: msg["reply_to_message"].as_object().map(|m| QuotedMessage {
        text: m["text"].as_str().unwrap_or("").to_string(),
        sender_name: m["from"]["first_name"].as_str().map(String::from),
        message_id: m["message_id"].as_i64(),
    }),
    message_id: msg["message_id"].as_i64(),
    timestamp: Some(Local::now()),
};

// Process
let response = self.runner
    .process_message_with_context(
        &session_key, &text, &msg_ctx, &self.capabilities(), false
    ).await?;

// Handle response — clean and simple
if response.is_silent { return Ok(()); }

if let Some(voice) = &response.voice_text {
    self.send_voice_response(chat_id, voice, response.reply_to).await?;
} else {
    self.send_message(chat_id, &response.text, response.reply_to).await?;
}

if let Some(emoji) = &response.reaction {
    self.react(chat_id, msg_ctx.message_id.unwrap(), emoji).await?;
}
```

## Migration Path

1. **Phase 1**: Add `context.rs` with all new types. No behavior change.
2. **Phase 2**: Extend `Channel` trait with `capabilities()` (default impl for backward compat).
3. **Phase 3**: Add `process_message_with_context` to AgentRunner. Old `process_message` delegates to it.
4. **Phase 4**: Refactor `workspace.rs` prompt builder into modular sections.
5. **Phase 5**: Update Telegram to construct `MessageContext` and use new API.
6. **Phase 6**: Add `ProcessedResponse` post-processing. Remove scattered logic from telegram.rs.
7. **Phase 7**: Add `RuntimeContext` population at startup.

Each phase is independently testable and deployable.

## Files Changed

- `src/context.rs` — NEW: all context types + ProcessedResponse
- `src/channels/mod.rs` — extend Channel trait
- `src/channels/telegram.rs` — construct MessageContext, use ProcessedResponse
- `src/agent.rs` — add process_message_with_context
- `src/workspace.rs` — modular prompt builder
- `src/main.rs` — construct RuntimeContext at startup
