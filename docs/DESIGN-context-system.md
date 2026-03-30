# Design: Context System Refactor

## Problem

RustClaw's system prompt is a hardcoded string in `workspace.rs`. Channel-specific context (sender info, reply-to messages, capabilities) doesn't flow through to the LLM. The `process_message` interface only passes `(session_key, user_message, user_id, channel)` as flat strings — no structured metadata.

## Design Goals

1. **Structured message metadata** flows from channel → agent → LLM context
2. **Channel declares its capabilities** once at startup
3. **Response post-processing** is unified and channel-agnostic
4. **System prompt composition** is modular, not one giant string
5. **Backward compatible** — existing `process_message` still works

## Architecture

### Key Design Decisions

1. **MessageContext goes in user message prefix, NOT system prompt** — system prompt is session-level; sender info changes per message. Inject as prefix like OpenClaw does.
2. **ChannelCapabilities stored on AgentRunner at startup** — not passed per-message.
3. **Voice mode stays in channel layer** — ProcessedResponse handles VOICE: prefix only; per-chat voice mode toggle is Telegram-specific state.
4. **Streaming path untouched** — streaming chunks can't be post-processed as a whole. Voice mode check happens at channel level after stream completes.

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

/// Channel capability declaration — set once at startup
#[derive(Debug, Clone)]
pub struct ChannelCapabilities {
    pub name: String,
    pub supports_reactions: bool,
    pub supports_inline_buttons: bool,
    pub supports_voice: bool,
    pub supports_reply_to: bool,
    pub supports_typing: bool,
    pub supports_markdown: bool,
    pub supports_tables: bool,
    pub max_message_length: usize,
    pub format_notes: Vec<String>,
}

/// Runtime info — populated once at startup
#[derive(Debug, Clone)]
pub struct RuntimeContext {
    pub os: String,
    pub arch: String,
    pub version: String,
    pub model: String,
    pub hostname: String,
}

/// Processed LLM response with extracted control signals
#[derive(Debug)]
pub struct ProcessedResponse {
    pub text: String,
    pub reply_to: Option<i64>,
    pub voice_text: Option<String>,
    pub is_silent: bool,
}

impl ProcessedResponse {
    pub fn from_raw(raw: &str) -> Self {
        let trimmed = raw.trim();
        let is_silent = trimmed == "NO_REPLY" || trimmed == "HEARTBEAT_OK";
        let (text, reply_to) = Self::extract_reply_tag(trimmed);
        let voice_text = Self::extract_voice_text(&text);
        
        ProcessedResponse {
            text: voice_text.clone().unwrap_or(text),
            reply_to,
            voice_text,
            is_silent,
        }
    }
}
```

### Message Context as User Message Prefix

NOT in system prompt. Channel constructs prefix:

```
[Telegram potato oneB (@potatosoupup) id:7539582820]
> Quoted message text here
> — quoted sender name

User's actual message
```

AgentRunner prepends this to user_message before sending to LLM.

### Channel Trait Extension

```rust
pub trait Channel: Send + Sync {
    fn name(&self) -> &str;
    fn capabilities(&self) -> ChannelCapabilities;  // NEW
    async fn start(&self, runner: Arc<AgentRunner>) -> anyhow::Result<()>;
}
```

### AgentRunner Changes

```rust
// Store capabilities at startup
pub struct AgentRunner {
    channel_caps: RwLock<Option<ChannelCapabilities>>,
    runtime_ctx: RuntimeContext,
    // ... existing fields
}

// New entry point
pub async fn process_message_with_context(
    &self,
    session_key: &str,
    user_message: &str,
    msg_ctx: &MessageContext,
    is_heartbeat: bool,
) -> anyhow::Result<ProcessedResponse>

// Old process_message delegates to new one
```

### Modular System Prompt Builder

```rust
impl Workspace {
    pub fn build_system_prompt_full(
        &self,
        runtime: &RuntimeContext,
        channel: &ChannelCapabilities,
        is_heartbeat: bool,
    ) -> String {
        let mut sections = Vec::new();
        sections.push(self.build_runtime_section(runtime));
        sections.push(self.build_behavioral_rules());
        sections.push(self.build_channel_section(channel));
        sections.push(self.build_voice_section());
        sections.push(self.build_memory_rules());
        sections.push(self.build_workspace_files());
        sections.push(self.build_skills_section(is_heartbeat, None));
        sections.push(self.build_daily_notes());
        sections.join("\n\n")
    }
}
```

Note: MessageContext NOT in system prompt — it's prepended to user message.

### Telegram Integration (simplified)

```rust
// Construct context
let msg_ctx = MessageContext {
    sender_id: Some(user_id.to_string()),
    sender_name: msg["from"]["first_name"].as_str().map(String::from),
    sender_username: msg["from"]["username"].as_str().map(String::from),
    chat_type: if is_group { ChatType::Group { ... } } else { ChatType::Direct },
    reply_to: msg["reply_to_message"].as_object().map(|m| QuotedMessage { ... }),
    message_id: msg["message_id"].as_i64(),
};

// Process
let response = self.runner
    .process_message_with_context(&session_key, &text, &msg_ctx, false)
    .await?;

// Handle — channel applies voice mode ON TOP of ProcessedResponse
if response.is_silent { return Ok(()); }

let use_voice = response.voice_text.is_some() || self.is_voice_mode(chat_id).await;
if use_voice {
    let voice_text = response.voice_text.as_ref().unwrap_or(&response.text);
    self.send_voice_response(chat_id, voice_text, response.reply_to).await?;
} else {
    self.send_message(chat_id, &response.text, response.reply_to).await?;
}
```

## Phases (4 phases, compressed)

### Phase A: Types + Runtime
- Create `src/context.rs` with all types
- Populate `RuntimeContext` at startup in `main.rs`
- No behavior change

### Phase B: Channel trait + Prompt builder
- Add `capabilities()` to Channel trait (default impl)
- Implement for Telegram
- Store `ChannelCapabilities` on AgentRunner
- Refactor `workspace.rs` into modular `build_system_prompt_full()`

### Phase C: AgentRunner interface
- Add `process_message_with_context()` returning `ProcessedResponse`
- Old `process_message()` delegates to new one
- MessageContext → user message prefix formatting

### Phase D: Telegram integration + Response processing
- Telegram constructs `MessageContext` (parse sender, reply_to_message)
- Use `ProcessedResponse` for response handling
- Remove scattered VOICE/NO_REPLY logic from telegram.rs
- Voice mode applied at channel layer on top of ProcessedResponse

## Files Changed

- `src/context.rs` — NEW
- `src/channels/mod.rs` — extend Channel trait
- `src/channels/telegram.rs` — MessageContext construction, ProcessedResponse handling
- `src/agent.rs` — process_message_with_context
- `src/workspace.rs` — modular prompt builder
- `src/main.rs` — RuntimeContext
