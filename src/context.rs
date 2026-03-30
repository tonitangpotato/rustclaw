//! Context types for structured message metadata, channel capabilities, and response processing.

use chrono::Local;

/// Per-message metadata from the channel.
#[derive(Debug, Clone, Default)]
pub struct MessageContext {
    pub sender_id: Option<String>,
    pub sender_name: Option<String>,
    pub sender_username: Option<String>,
    pub chat_type: ChatType,
    pub reply_to: Option<QuotedMessage>,
    pub message_id: Option<i64>,
}

impl MessageContext {
    /// Format as a user message prefix (injected before the actual message).
    pub fn format_prefix(&self, channel_name: &str) -> String {
        let mut parts = Vec::new();

        // Sender line
        let mut sender = format!("[{}", channel_name.to_uppercase());
        if let Some(name) = &self.sender_name {
            sender.push(' ');
            sender.push_str(name);
        }
        if let Some(username) = &self.sender_username {
            sender.push_str(&format!(" (@{})", username));
        }
        if let Some(id) = &self.sender_id {
            sender.push_str(&format!(" id:{}", id));
        }
        // Chat type
        match &self.chat_type {
            ChatType::Direct => {}
            ChatType::Group { title } => {
                if let Some(t) = title {
                    sender.push_str(&format!(" in group \"{}\"", t));
                } else {
                    sender.push_str(" in group");
                }
            }
        }
        // Timestamp
        sender.push_str(&format!(" {}]", Local::now().format("%a %Y-%m-%d %H:%M %Z")));
        parts.push(sender);

        // Quoted message
        if let Some(reply) = &self.reply_to {
            let quoted_sender = reply
                .sender_name
                .as_deref()
                .unwrap_or("unknown");
            parts.push(format!(
                "Replying to {}:\n> {}",
                quoted_sender,
                reply.text.lines().collect::<Vec<_>>().join("\n> ")
            ));
        }

        if parts.is_empty() {
            String::new()
        } else {
            parts.join("\n") + "\n\n"
        }
    }
}

/// Chat type: direct message or group.
#[derive(Debug, Clone, Default)]
pub enum ChatType {
    #[default]
    Direct,
    Group {
        title: Option<String>,
    },
}

/// A quoted/replied-to message.
#[derive(Debug, Clone)]
pub struct QuotedMessage {
    pub text: String,
    pub sender_name: Option<String>,
    pub message_id: Option<i64>,
}

/// Channel capability declaration — set once at startup.
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

impl Default for ChannelCapabilities {
    fn default() -> Self {
        Self {
            name: "cli".into(),
            supports_reactions: false,
            supports_inline_buttons: false,
            supports_voice: false,
            supports_reply_to: false,
            supports_typing: false,
            supports_markdown: true,
            supports_tables: true,
            max_message_length: 65536,
            format_notes: vec![],
        }
    }
}

impl ChannelCapabilities {
    /// Format capabilities for system prompt injection.
    pub fn format_for_prompt(&self) -> String {
        let mut caps = Vec::new();
        if self.supports_reactions {
            caps.push("reactions");
        }
        if self.supports_inline_buttons {
            caps.push("inline_buttons");
        }
        if self.supports_voice {
            caps.push("voice_messages");
        }
        if self.supports_reply_to {
            caps.push("reply_to");
        }
        if self.supports_typing {
            caps.push("typing_indicator");
        }
        if self.supports_markdown {
            caps.push("markdown");
        }
        if self.supports_tables {
            caps.push("tables");
        }

        let mut s = format!(
            "## Channel: {}\nCapabilities: {}\nMax message length: {}\n",
            self.name,
            caps.join(", "),
            self.max_message_length,
        );

        for note in &self.format_notes {
            s.push_str(&format!("- {}\n", note));
        }

        s
    }
}

/// Runtime info — populated once at startup.
#[derive(Debug, Clone)]
pub struct RuntimeContext {
    pub os: String,
    pub arch: String,
    pub version: String,
    pub model: String,
    pub hostname: String,
}

impl RuntimeContext {
    /// Detect runtime context from the current environment.
    pub fn detect(model: &str) -> Self {
        let os = std::process::Command::new("uname")
            .arg("-sr")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|_| "unknown".into());

        let arch = std::env::consts::ARCH.to_string();

        let hostname = std::process::Command::new("hostname")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|_| "unknown".into());

        Self {
            os,
            arch,
            version: env!("CARGO_PKG_VERSION").to_string(),
            model: model.to_string(),
            hostname,
        }
    }

    /// Format for system prompt injection.
    pub fn format_for_prompt(&self) -> String {
        format!(
            "Runtime: RustClaw v{} | {} ({}) | host={} | model={}",
            self.version, self.os, self.arch, self.hostname, self.model,
        )
    }
}

/// Processed LLM response with extracted control signals.
#[derive(Debug)]
pub struct ProcessedResponse {
    /// The text to display (voice text extracted if applicable).
    pub text: String,
    /// Reply to a specific message ID.
    pub reply_to: Option<i64>,
    /// If set, the response should be sent as voice with this text.
    pub voice_text: Option<String>,
    /// Whether the response should be suppressed (NO_REPLY / HEARTBEAT_OK).
    pub is_silent: bool,
}

impl ProcessedResponse {
    /// Parse raw LLM output into a structured response.
    pub fn from_raw(raw: &str) -> Self {
        let trimmed = raw.trim();

        let is_silent = trimmed == "NO_REPLY" || trimmed == "HEARTBEAT_OK";

        let (text, reply_to) = Self::extract_reply_tag(trimmed);
        let voice_text = Self::extract_voice_text(&text);

        ProcessedResponse {
            text: voice_text.clone().unwrap_or_else(|| text.clone()),
            reply_to,
            voice_text,
            is_silent,
        }
    }

    /// Extract `[[reply_to:123]]` tag from response.
    fn extract_reply_tag(text: &str) -> (String, Option<i64>) {
        // Match [[reply_to:123]] or [[ reply_to: 123 ]] at start of text
        let re = regex::Regex::new(r"^\[\[\s*reply_to:\s*(\d+)\s*\]\]\s*").unwrap();
        if let Some(caps) = re.captures(text) {
            let id = caps[1].parse::<i64>().ok();
            let rest = re.replace(text, "").to_string();
            (rest, id)
        } else {
            (text.to_string(), None)
        }
    }

    /// Extract VOICE: prefix or \nVOICE: anywhere in text.
    fn extract_voice_text(text: &str) -> Option<String> {
        let trimmed = text.trim();

        // Check VOICE: at start
        if let Some(rest) = trimmed.strip_prefix("VOICE:") {
            return Some(rest.trim().to_string());
        }
        // Check 🔊 at start
        if let Some(rest) = trimmed.strip_prefix("🔊") {
            return Some(rest.trim().to_string());
        }
        // Check \nVOICE: anywhere (LLM sometimes puts preamble before VOICE:)
        if let Some(pos) = trimmed.find("\nVOICE:") {
            let after = &trimmed[pos + 7..];
            return Some(after.trim().to_string());
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_processed_response_no_reply() {
        let r = ProcessedResponse::from_raw("NO_REPLY");
        assert!(r.is_silent);
    }

    #[test]
    fn test_processed_response_heartbeat() {
        let r = ProcessedResponse::from_raw("HEARTBEAT_OK");
        assert!(r.is_silent);
    }

    #[test]
    fn test_processed_response_voice() {
        let r = ProcessedResponse::from_raw("VOICE: Hello world");
        assert!(!r.is_silent);
        assert_eq!(r.voice_text, Some("Hello world".into()));
        assert_eq!(r.text, "Hello world");
    }

    #[test]
    fn test_processed_response_voice_in_middle() {
        let r = ProcessedResponse::from_raw("Some preamble\nVOICE: The actual voice text");
        assert_eq!(r.voice_text, Some("The actual voice text".into()));
    }

    #[test]
    fn test_processed_response_reply_tag() {
        let r = ProcessedResponse::from_raw("[[reply_to:12345]] Hello there");
        assert_eq!(r.reply_to, Some(12345));
        assert_eq!(r.text, "Hello there");
    }

    #[test]
    fn test_processed_response_plain() {
        let r = ProcessedResponse::from_raw("Just a normal message");
        assert!(!r.is_silent);
        assert!(r.voice_text.is_none());
        assert!(r.reply_to.is_none());
        assert_eq!(r.text, "Just a normal message");
    }

    #[test]
    fn test_message_context_prefix_direct() {
        let ctx = MessageContext {
            sender_name: Some("potato".into()),
            sender_username: Some("potatosoupup".into()),
            sender_id: Some("123".into()),
            chat_type: ChatType::Direct,
            ..Default::default()
        };
        let prefix = ctx.format_prefix("telegram");
        assert!(prefix.contains("TELEGRAM potato (@potatosoupup) id:123"));
    }

    #[test]
    fn test_message_context_prefix_group() {
        let ctx = MessageContext {
            sender_name: Some("potato".into()),
            chat_type: ChatType::Group {
                title: Some("Test Group".into()),
            },
            ..Default::default()
        };
        let prefix = ctx.format_prefix("telegram");
        assert!(prefix.contains("in group \"Test Group\""));
    }

    #[test]
    fn test_message_context_prefix_with_reply() {
        let ctx = MessageContext {
            sender_name: Some("potato".into()),
            reply_to: Some(QuotedMessage {
                text: "Original message".into(),
                sender_name: Some("bot".into()),
                message_id: Some(999),
            }),
            ..Default::default()
        };
        let prefix = ctx.format_prefix("telegram");
        assert!(prefix.contains("Replying to bot:"));
        assert!(prefix.contains("> Original message"));
    }

    #[test]
    fn test_channel_capabilities_format() {
        let caps = ChannelCapabilities {
            name: "telegram".into(),
            supports_reactions: true,
            supports_voice: true,
            supports_tables: false,
            format_notes: vec!["Use bullet lists instead of tables".into()],
            ..Default::default()
        };
        let s = caps.format_for_prompt();
        assert!(s.contains("Channel: telegram"));
        assert!(s.contains("reactions"));
        assert!(s.contains("voice_messages"));
        // "tables" capability should not be listed (supports_tables=false)
        assert!(!s.contains("tables,") && !s.contains(", tables"));
        assert!(s.contains("Use bullet lists"));
    }

    #[test]
    fn test_runtime_context_format() {
        let rt = RuntimeContext {
            os: "Darwin 24.6.0".into(),
            arch: "aarch64".into(),
            version: "0.1.0".into(),
            model: "claude-opus-4-6".into(),
            hostname: "test-host".into(),
        };
        let s = rt.format_for_prompt();
        assert!(s.contains("RustClaw v0.1.0"));
        assert!(s.contains("Darwin"));
        assert!(s.contains("claude-opus-4-6"));
    }
}
