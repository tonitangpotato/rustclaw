//! Context types for structured message metadata, channel capabilities, and response processing.
//!
//! ## Envelope (ISS-021)
//!
//! `Envelope` is the structured per-message metadata carried alongside — not inside —
//! user content. Deriving `Serialize`/`Deserialize` lets us persist an envelope to
//! `engramai::StorageMeta::user_metadata` under the `envelope` key, enabling
//! context-aware recall without header-string parsing.
//!
//! The legacy name `MessageContext` (a type alias) was removed in Phase 4 — all
//! call sites use `Envelope` directly.

use chrono::Local;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Channels whose headers are recognised by [`Envelope::strip_from_content`].
///
/// The whitelist is deliberately closed: any prefix `[FOO ...]` where `FOO` is
/// not in this list is treated as legitimate content, not a header. This makes
/// `strip_from_content` safe to apply to arbitrary memory records — a message
/// whose body genuinely starts with `[` (e.g. a user pasting JSON or a tag
/// like `[RFC]`) will never be mis-identified as a channel header.
pub const KNOWN_CHANNELS: &[&str] = &[
    "TELEGRAM", "DISCORD", "SLACK", "SIGNAL", "MATRIX", "WHATSAPP",
];

/// Compiled channel-header regex (lazy, single compile per process).
///
/// Pattern breakdown:
///   `^\[(TELEGRAM|DISCORD|...) ` — anchored channel label + space
///   `([^\]\n]+)`                  — header body: no embedded `]` or newline
///   `\]\n`                        — closing bracket + single newline
///
/// Note: we match only up to `]\n` (not `]\n\n`) because the line after the
/// header may be either a blank line (no reply) or `Replying to ...` (reply
/// block present). The caller distinguishes these cases after the match.
fn header_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        let channels = KNOWN_CHANNELS.join("|");
        let pattern = format!(r"^\[({}) ([^\]\n]+)\]\n", channels);
        Regex::new(&pattern).expect("header_regex: hardcoded pattern must compile")
    })
}

/// Compiled reply-block regex for the optional quoted-message section that
/// follows the header line (see `Envelope::render_for_prompt`).
///
/// Matches `Replying to <sender>[ (@user)][ (msg_id:N)]:\n> <text...>\n\n`
/// where the quoted body is one or more lines each prefixed with `> `.
fn reply_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Non-greedy quoted body: consume `> ...` lines up to the first blank.
        Regex::new(r"^Replying to [^\n]+:\n(?:> [^\n]*\n)+\n")
            .expect("reply_regex: hardcoded pattern must compile")
    })
}

/// Result of a successful [`Envelope::strip_from_content`] match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StripResult {
    /// The channel label (e.g. `"TELEGRAM"`). Always upper-case.
    pub channel: String,
    /// Raw header body — everything between `[CHANNEL ` and `]`. Not parsed
    /// into sub-fields; callers that need sender_id/timestamp should parse it
    /// themselves. We keep it raw on purpose: the on-disk format has drifted
    /// across versions and any structured parse would have to tolerate every
    /// historical variant, which is more risk than Phase 5a warrants.
    pub header_body: String,
    /// Raw reply block (if present) — verbatim `Replying to ...\n> ...\n\n`.
    pub reply_block: Option<String>,
    /// Content with header (and optional reply block) removed. All other bytes
    /// preserved byte-for-byte — no trimming, no normalisation.
    pub stripped_content: String,
}

/// Per-message metadata from the channel.
///
/// This is the "side channel" for who/where/when context. It is **never**
/// concatenated into the user message string; instead it is rendered into the
/// system prompt at the appropriate boundary (via `render_for_prompt`) and
/// persisted as JSON on memory records (via `StorageMeta::user_metadata`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Envelope {
    pub sender_id: Option<String>,
    pub sender_name: Option<String>,
    pub sender_username: Option<String>,
    pub chat_type: ChatType,
    pub reply_to: Option<QuotedMessage>,
    pub message_id: Option<i64>,
}

impl Envelope {
    /// Format as a user message prefix (injected before the actual message).
    pub fn render_for_prompt(&self, channel_name: &str) -> String {
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

        // Quoted message (with message_id so LLM can reference it)
        if let Some(reply) = &self.reply_to {
            let quoted_sender = reply
                .sender_name
                .as_deref()
                .unwrap_or("unknown");
            let msg_id_str = reply.message_id
                .map(|id| format!(" (msg_id:{})", id))
                .unwrap_or_default();
            // Include sender username if available for better identification
            let username_str = reply.sender_username
                .as_ref()
                .map(|u| format!(" (@{})", u))
                .unwrap_or_default();
            parts.push(format!(
                "Replying to {}{}{}:\n> {}",
                quoted_sender,
                username_str,
                msg_id_str,
                reply.text.lines().collect::<Vec<_>>().join("\n> ")
            ));
        }

        if parts.is_empty() {
            String::new()
        } else {
            parts.join("\n") + "\n\n"
        }
    }

    /// Attempt to recognise and strip a legacy channel-header prefix from
    /// `content`. Returns `None` if the content does not start with a
    /// recognised header — never mutates or guesses.
    ///
    /// This is the inverse of [`Envelope::render_for_prompt`] for historical
    /// records stored before ISS-021 Phase 2+3, when channel code concatenated
    /// the rendered prefix into the user message string. The function is
    /// intentionally conservative:
    ///
    /// * Channel label must be in [`KNOWN_CHANNELS`].
    /// * Header body must not contain `]` or newline (tight character class).
    /// * A blank line (`\n\n`) must separate header from body.
    /// * Reply blocks are only stripped when they immediately follow the
    ///   header and end with a blank line.
    /// * Everything else is left byte-identical.
    ///
    /// The returned [`StripResult`] carries the raw header body; callers that
    /// need to persist structured envelope fields must parse it themselves.
    /// Phase 5a only reports counts + previews, so raw is sufficient.
    pub fn strip_from_content(content: &str) -> Option<StripResult> {
        // Phase 1: match the header line. `header_regex` matches through the
        // single newline after `]`, leaving us positioned at either the body
        // (blank line next) or a reply block (`Replying to ...` next).
        let caps = header_regex().captures(content)?;
        let header_match = caps.get(0)?;
        debug_assert_eq!(header_match.start(), 0);
        let after_header = header_match.end();
        let channel = caps.get(1)?.as_str().to_string();
        let header_body = caps.get(2)?.as_str().to_string();

        let remainder = &content[after_header..];

        // Phase 2: distinguish reply-block vs blank-line-then-body.
        // Case A: remainder starts with `\n` → header line was followed by
        //         a second `\n` (i.e. original had `]\n\n`). Body begins
        //         after that blank.
        // Case B: remainder starts with `Replying to ` → try to consume the
        //         full reply block, which ends with `\n\n`. Body begins after.
        // Case C: anything else → malformed structure, refuse to strip.
        if let Some(body) = remainder.strip_prefix('\n') {
            // Case A — no reply block.
            Some(StripResult {
                channel,
                header_body,
                reply_block: None,
                stripped_content: body.to_string(),
            })
        } else if remainder.starts_with("Replying to ") {
            // Case B — try to consume reply block.
            let m = reply_regex().find(remainder)?;
            debug_assert_eq!(m.start(), 0);
            let reply_block = m.as_str().to_string();
            let body = &remainder[m.end()..];
            Some(StripResult {
                channel,
                header_body,
                reply_block: Some(reply_block),
                stripped_content: body.to_string(),
            })
        } else {
            // Case C — structure doesn't match either shape. Refuse.
            None
        }
    }
}

/// Chat type: direct message or group.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum ChatType {
    #[default]
    Direct,
    Group {
        title: Option<String>,
    },
}

/// A quoted/replied-to message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotedMessage {
    pub text: String,
    pub sender_name: Option<String>,
    pub sender_username: Option<String>,
    pub sender_id: Option<String>,
    pub message_id: Option<i64>,
}

impl QuotedMessage {
    /// Parse a QuotedMessage from a Telegram `reply_to_message` JSON value.
    ///
    /// Extracts text, sender info, and message ID from the Telegram message object.
    /// Handles text messages, captions, stickers, photos, voice, documents, video, and audio.
    pub fn from_telegram_json(msg: &serde_json::Value) -> Option<Self> {
        let text = msg.get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        let caption = msg.get("caption")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        // Use text if available, otherwise caption (for photos/documents with captions)
        let content = if !text.is_empty() {
            text
        } else if !caption.is_empty() {
            caption
        } else {
            // Describe non-text message types
            if msg.get("sticker").is_some() {
                let emoji = msg["sticker"]["emoji"].as_str().unwrap_or("🏷");
                format!("[Sticker: {}]", emoji)
            } else if msg.get("photo").is_some() {
                "[Photo]".to_string()
            } else if msg.get("voice").is_some() {
                "[Voice message]".to_string()
            } else if msg.get("document").is_some() {
                let name = msg["document"]["file_name"].as_str().unwrap_or("file");
                format!("[Document: {}]", name)
            } else if msg.get("video").is_some() {
                "[Video]".to_string()
            } else if msg.get("audio").is_some() {
                "[Audio]".to_string()
            } else {
                "[Message]".to_string()
            }
        };

        let sender_name = msg.get("from")
            .and_then(|f| f.get("first_name"))
            .and_then(|n| n.as_str())
            .map(String::from);

        let sender_username = msg.get("from")
            .and_then(|f| f.get("username"))
            .and_then(|u| u.as_str())
            .map(String::from);

        let sender_id = msg.get("from")
            .and_then(|f| f.get("id"))
            .and_then(|id| id.as_i64())
            .map(|id| id.to_string());

        let message_id = msg.get("message_id")
            .and_then(|id| id.as_i64());

        Some(QuotedMessage {
            text: content,
            sender_name,
            sender_username,
            sender_id,
            message_id,
        })
    }

    /// Parse a QuotedMessage from a Discord referenced message.
    ///
    /// Takes the author name, author ID, message content, and message ID
    /// from a Discord message struct.
    pub fn from_discord(
        author_name: &str,
        author_id: &str,
        content: &str,
        message_id: u64,
    ) -> Self {
        // Discord content may be empty if it's an attachment/embed-only message
        let text = if content.is_empty() {
            "[Attachment/Embed]".to_string()
        } else {
            content.to_string()
        };

        QuotedMessage {
            text,
            sender_name: Some(author_name.to_string()),
            sender_username: None,
            sender_id: Some(author_id.to_string()),
            message_id: Some(message_id as i64),
        }
    }

    /// Return a copy with text truncated to a maximum number of characters.
    /// Useful for keeping context prefixes concise.
    pub fn truncated(&self, max_chars: usize) -> Self {
        let truncated_text = if self.text.len() > max_chars {
            let boundary = self.text.floor_char_boundary(max_chars);
            format!("{}…", &self.text[..boundary])
        } else {
            self.text.clone()
        };
        QuotedMessage {
            text: truncated_text,
            sender_name: self.sender_name.clone(),
            sender_username: self.sender_username.clone(),
            sender_id: self.sender_id.clone(),
            message_id: self.message_id,
        }
    }
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
    /// The text to display.
    pub text: String,
    /// Reply to a specific message ID.
    pub reply_to: Option<i64>,
    /// Whether the response should be suppressed (NO_REPLY / HEARTBEAT_OK).
    pub is_silent: bool,
}

impl ProcessedResponse {
    /// Parse raw LLM output into a structured response.
    pub fn from_raw(raw: &str) -> Self {
        let trimmed = raw.trim();

        let is_silent = trimmed == "NO_REPLY" || trimmed == "HEARTBEAT_OK";

        let (text, reply_to) = Self::extract_reply_tag(trimmed);

        ProcessedResponse {
            text,
            reply_to,
            is_silent,
        }
    }

    /// Extract `[[reply_to:123]]` tag from response.
    fn extract_reply_tag(text: &str) -> (String, Option<i64>) {
        let re = regex::Regex::new(r"^\[\[\s*reply_to:\s*(\d+)\s*\]\]\s*").unwrap();
        if let Some(caps) = re.captures(text) {
            let id = caps[1].parse::<i64>().ok();
            let rest = re.replace(text, "").to_string();
            (rest, id)
        } else {
            (text.to_string(), None)
        }
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
    fn test_processed_response_reply_tag() {
        let r = ProcessedResponse::from_raw("[[reply_to:12345]] Hello there");
        assert_eq!(r.reply_to, Some(12345));
        assert_eq!(r.text, "Hello there");
    }

    #[test]
    fn test_processed_response_plain() {
        let r = ProcessedResponse::from_raw("Just a normal message");
        assert!(!r.is_silent);
        assert!(r.reply_to.is_none());
        assert_eq!(r.text, "Just a normal message");
    }

    #[test]
    fn test_message_context_prefix_direct() {
        let ctx = Envelope {
            sender_name: Some("potato".into()),
            sender_username: Some("potatosoupup".into()),
            sender_id: Some("123".into()),
            chat_type: ChatType::Direct,
            ..Default::default()
        };
        let prefix = ctx.render_for_prompt("telegram");
        assert!(prefix.contains("TELEGRAM potato (@potatosoupup) id:123"));
    }

    #[test]
    fn test_message_context_prefix_group() {
        let ctx = Envelope {
            sender_name: Some("potato".into()),
            chat_type: ChatType::Group {
                title: Some("Test Group".into()),
            },
            ..Default::default()
        };
        let prefix = ctx.render_for_prompt("telegram");
        assert!(prefix.contains("in group \"Test Group\""));
    }

    #[test]
    fn test_message_context_prefix_with_reply() {
        let ctx = Envelope {
            sender_name: Some("potato".into()),
            reply_to: Some(QuotedMessage {
                text: "Original message".into(),
                sender_name: Some("bot".into()),
                sender_username: None,
                sender_id: None,
                message_id: Some(999),
            }),
            ..Default::default()
        };
        let prefix = ctx.render_for_prompt("telegram");
        assert!(prefix.contains("Replying to bot (msg_id:999):"));
        assert!(prefix.contains("> Original message"));
    }

    #[test]
    fn test_message_context_prefix_with_reply_username() {
        let ctx = Envelope {
            sender_name: Some("potato".into()),
            reply_to: Some(QuotedMessage {
                text: "Hello there".into(),
                sender_name: Some("bot".into()),
                sender_username: Some("mybot".into()),
                sender_id: None,
                message_id: Some(42),
            }),
            ..Default::default()
        };
        let prefix = ctx.render_for_prompt("telegram");
        assert!(prefix.contains("Replying to bot (@mybot) (msg_id:42):"));
        assert!(prefix.contains("> Hello there"));
    }

    #[test]
    fn test_message_context_prefix_reply_multiline() {
        let ctx = Envelope {
            sender_name: Some("user".into()),
            reply_to: Some(QuotedMessage {
                text: "Line one\nLine two\nLine three".into(),
                sender_name: Some("other".into()),
                sender_username: None,
                sender_id: None,
                message_id: None,
            }),
            ..Default::default()
        };
        let prefix = ctx.render_for_prompt("discord");
        assert!(prefix.contains("> Line one\n> Line two\n> Line three"));
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

    // ===== QuotedMessage::from_telegram_json tests =====

    #[test]
    fn test_telegram_reply_text_message() {
        let json = serde_json::json!({
            "message_id": 123,
            "from": {
                "id": 456,
                "first_name": "Alice",
                "username": "alice123"
            },
            "text": "Hello, world!"
        });
        let quoted = QuotedMessage::from_telegram_json(&json).unwrap();
        assert_eq!(quoted.text, "Hello, world!");
        assert_eq!(quoted.sender_name.as_deref(), Some("Alice"));
        assert_eq!(quoted.sender_username.as_deref(), Some("alice123"));
        assert_eq!(quoted.sender_id.as_deref(), Some("456"));
        assert_eq!(quoted.message_id, Some(123));
    }

    #[test]
    fn test_telegram_reply_caption_message() {
        let json = serde_json::json!({
            "message_id": 789,
            "from": { "id": 100, "first_name": "Bob" },
            "photo": [{"file_id": "abc"}],
            "caption": "Look at this photo!"
        });
        let quoted = QuotedMessage::from_telegram_json(&json).unwrap();
        assert_eq!(quoted.text, "Look at this photo!");
        assert_eq!(quoted.sender_name.as_deref(), Some("Bob"));
        assert_eq!(quoted.sender_username, None);
    }

    #[test]
    fn test_telegram_reply_sticker() {
        let json = serde_json::json!({
            "message_id": 50,
            "from": { "id": 1, "first_name": "Eve" },
            "sticker": { "emoji": "😂", "file_id": "stk1" }
        });
        let quoted = QuotedMessage::from_telegram_json(&json).unwrap();
        assert_eq!(quoted.text, "[Sticker: 😂]");
    }

    #[test]
    fn test_telegram_reply_photo_no_caption() {
        let json = serde_json::json!({
            "message_id": 51,
            "from": { "id": 2, "first_name": "Frank" },
            "photo": [{"file_id": "ph1"}]
        });
        let quoted = QuotedMessage::from_telegram_json(&json).unwrap();
        assert_eq!(quoted.text, "[Photo]");
    }

    #[test]
    fn test_telegram_reply_voice() {
        let json = serde_json::json!({
            "message_id": 52,
            "from": { "id": 3, "first_name": "Grace" },
            "voice": { "file_id": "v1", "duration": 5 }
        });
        let quoted = QuotedMessage::from_telegram_json(&json).unwrap();
        assert_eq!(quoted.text, "[Voice message]");
    }

    #[test]
    fn test_telegram_reply_document() {
        let json = serde_json::json!({
            "message_id": 53,
            "from": { "id": 4, "first_name": "Hank" },
            "document": { "file_id": "d1", "file_name": "report.pdf" }
        });
        let quoted = QuotedMessage::from_telegram_json(&json).unwrap();
        assert_eq!(quoted.text, "[Document: report.pdf]");
    }

    #[test]
    fn test_telegram_reply_video() {
        let json = serde_json::json!({
            "message_id": 54,
            "from": { "id": 5, "first_name": "Ivy" },
            "video": { "file_id": "vid1" }
        });
        let quoted = QuotedMessage::from_telegram_json(&json).unwrap();
        assert_eq!(quoted.text, "[Video]");
    }

    #[test]
    fn test_telegram_reply_audio() {
        let json = serde_json::json!({
            "message_id": 55,
            "from": { "id": 6, "first_name": "Jack" },
            "audio": { "file_id": "aud1" }
        });
        let quoted = QuotedMessage::from_telegram_json(&json).unwrap();
        assert_eq!(quoted.text, "[Audio]");
    }

    #[test]
    fn test_telegram_reply_empty_message() {
        let json = serde_json::json!({
            "message_id": 56,
            "from": { "id": 7, "first_name": "Kim" }
        });
        let quoted = QuotedMessage::from_telegram_json(&json).unwrap();
        assert_eq!(quoted.text, "[Message]");
    }

    #[test]
    fn test_telegram_reply_no_sender() {
        let json = serde_json::json!({
            "message_id": 57,
            "text": "Anonymous message"
        });
        let quoted = QuotedMessage::from_telegram_json(&json).unwrap();
        assert_eq!(quoted.text, "Anonymous message");
        assert!(quoted.sender_name.is_none());
        assert!(quoted.sender_username.is_none());
        assert!(quoted.sender_id.is_none());
    }

    // ===== QuotedMessage::from_discord tests =====

    #[test]
    fn test_discord_reply_text() {
        let quoted = QuotedMessage::from_discord("Alice", "12345", "Hello Discord!", 99999);
        assert_eq!(quoted.text, "Hello Discord!");
        assert_eq!(quoted.sender_name.as_deref(), Some("Alice"));
        assert_eq!(quoted.sender_id.as_deref(), Some("12345"));
        assert_eq!(quoted.message_id, Some(99999));
    }

    #[test]
    fn test_discord_reply_empty_content() {
        let quoted = QuotedMessage::from_discord("Bob", "67890", "", 11111);
        assert_eq!(quoted.text, "[Attachment/Embed]");
        assert_eq!(quoted.sender_name.as_deref(), Some("Bob"));
    }

    // ===== QuotedMessage::truncated tests =====

    #[test]
    fn test_truncated_short_text() {
        let q = QuotedMessage {
            text: "Short".into(),
            sender_name: Some("A".into()),
            sender_username: None,
            sender_id: None,
            message_id: Some(1),
        };
        let t = q.truncated(100);
        assert_eq!(t.text, "Short");
    }

    #[test]
    fn test_truncated_long_text() {
        let q = QuotedMessage {
            text: "This is a very long message that should be truncated for brevity in the context prefix".into(),
            sender_name: Some("A".into()),
            sender_username: None,
            sender_id: None,
            message_id: Some(1),
        };
        let t = q.truncated(20);
        assert!(t.text.len() <= 24); // 20 chars + "…" (3 bytes)
        assert!(t.text.ends_with('…'));
    }

    // ===== Full integration: Telegram reply-to context formatting =====

    #[test]
    fn test_telegram_reply_context_integration() {
        let reply_json = serde_json::json!({
            "message_id": 100,
            "from": {
                "id": 200,
                "first_name": "RustClaw",
                "username": "rustclawbot"
            },
            "text": "Here's the info you requested"
        });
        let quoted = QuotedMessage::from_telegram_json(&reply_json).unwrap();
        let ctx = Envelope {
            sender_name: Some("potato".into()),
            sender_username: Some("potatosoupup".into()),
            sender_id: Some("300".into()),
            chat_type: ChatType::Group {
                title: Some("Dev Chat".into()),
            },
            reply_to: Some(quoted),
            message_id: Some(101),
        };
        let prefix = ctx.render_for_prompt("telegram");
        assert!(prefix.contains("TELEGRAM potato (@potatosoupup) id:300"));
        assert!(prefix.contains("in group \"Dev Chat\""));
        assert!(prefix.contains("Replying to RustClaw (@rustclawbot) (msg_id:100):"));
        assert!(prefix.contains("> Here's the info you requested"));
    }

    // ===== Full integration: Discord reply-to context formatting =====

    #[test]
    fn test_discord_reply_context_integration() {
        let quoted = QuotedMessage::from_discord(
            "SomeUser",
            "111222333",
            "What's the weather like?",
            444555666,
        );
        let ctx = Envelope {
            sender_name: Some("Replier".into()),
            sender_id: Some("777888999".into()),
            chat_type: ChatType::Group {
                title: Some("General".into()),
            },
            reply_to: Some(quoted),
            message_id: Some(444555667),
            ..Default::default()
        };
        let prefix = ctx.render_for_prompt("discord");
        assert!(prefix.contains("DISCORD Replier id:777888999"));
        assert!(prefix.contains("in group \"General\""));
        assert!(prefix.contains("Replying to SomeUser (msg_id:444555666):"));
        assert!(prefix.contains("> What's the weather like?"));
    }

    #[test]
    fn test_discord_reply_context_dm() {
        let quoted = QuotedMessage::from_discord(
            "BotName",
            "100200300",
            "I can help with that!",
            900800700,
        );
        let ctx = Envelope {
            sender_name: Some("User".into()),
            sender_id: Some("400500600".into()),
            chat_type: ChatType::Direct,
            reply_to: Some(quoted),
            ..Default::default()
        };
        let prefix = ctx.render_for_prompt("discord");
        assert!(prefix.contains("DISCORD User id:400500600"));
        assert!(!prefix.contains("in group"));
        assert!(prefix.contains("Replying to BotName (msg_id:900800700):"));
        assert!(prefix.contains("> I can help with that!"));
    }

    // ──────────────────────────────────────────────────────────────────
    // ISS-021 Phase 5a: strip_from_content()
    //
    // These cover every invariant the migration dry-run depends on:
    // channel whitelist, leading-`[` false positives, malformed headers,
    // multi-channel variants, reply-block preservation/preservation, UTF-8,
    // and the round-trip shape produced by `render_for_prompt`.
    // ──────────────────────────────────────────────────────────────────

    #[test]
    fn strip_matches_bare_telegram_header() {
        let content = "[TELEGRAM id:7539582820 Mon 2026-03-30 10:50 -04:00]\n\nhello world";
        let r = Envelope::strip_from_content(content).expect("should match");
        assert_eq!(r.channel, "TELEGRAM");
        assert_eq!(r.header_body, "id:7539582820 Mon 2026-03-30 10:50 -04:00");
        assert!(r.reply_block.is_none());
        assert_eq!(r.stripped_content, "hello world");
    }

    #[test]
    fn strip_matches_telegram_with_name_and_username() {
        let content = "[TELEGRAM potato (@potatosoupup) id:7539582820 Wed 2026-04-01 17:13 -04:00]\n\nactual message body";
        let r = Envelope::strip_from_content(content).expect("should match");
        assert_eq!(r.channel, "TELEGRAM");
        assert!(r.header_body.contains("potato (@potatosoupup)"));
        assert_eq!(r.stripped_content, "actual message body");
    }

    #[test]
    fn strip_matches_group_chat_header() {
        let content = "[TELEGRAM alice id:123 in group \"devs\" Mon 2026-04-20 09:00 -04:00]\n\nmorning";
        let r = Envelope::strip_from_content(content).expect("should match");
        assert_eq!(r.channel, "TELEGRAM");
        assert!(r.header_body.contains("in group \"devs\""));
        assert_eq!(r.stripped_content, "morning");
    }

    #[test]
    fn strip_matches_all_known_channels() {
        for channel in KNOWN_CHANNELS {
            let content = format!("[{} id:1 Mon 2026-01-01 00:00 UTC]\n\nbody", channel);
            let r = Envelope::strip_from_content(&content)
                .unwrap_or_else(|| panic!("channel {} should strip", channel));
            assert_eq!(r.channel, *channel);
            assert_eq!(r.stripped_content, "body");
        }
    }

    #[test]
    fn strip_rejects_unknown_channel() {
        // IRC is not in the whitelist — must pass through unchanged.
        let content = "[IRC nick!user@host Mon 2026-01-01]\n\nhello";
        assert!(Envelope::strip_from_content(content).is_none());
    }

    #[test]
    fn strip_rejects_content_starting_with_bracket_but_not_header() {
        // User legitimately pasted a JSON / tag / code snippet.
        let cases = &[
            "[RFC] Proposal: support foo\n\nbody",
            "[DRAFT]\n\nsome thoughts",
            "[{\"key\": \"value\"}]\n\nmore",
            "[not-a-channel] nope\n\nbody",
            "[telegram lowercase]\n\nbody", // case-sensitive — must not match
        ];
        for c in cases {
            assert!(
                Envelope::strip_from_content(c).is_none(),
                "should NOT strip: {:?}",
                c
            );
        }
    }

    #[test]
    fn strip_rejects_header_without_blank_line() {
        // Single \n after ] is insufficient; format demands \n\n.
        let content = "[TELEGRAM id:1 Mon 2026-01-01]\nbody";
        assert!(Envelope::strip_from_content(content).is_none());
    }

    #[test]
    fn strip_rejects_header_with_embedded_newline() {
        // Character class forbids \n inside the header body.
        let content = "[TELEGRAM id:1\nMon 2026-01-01]\n\nbody";
        assert!(Envelope::strip_from_content(content).is_none());
    }

    #[test]
    fn strip_rejects_header_with_embedded_bracket() {
        // Our `[^\]\n]+` class refuses embedded `]` inside the header body.
        // Here the first `]` closes the header but is followed by ` tail]`,
        // not by `\n` — so the header regex can't match at all.
        let content = "[TELEGRAM id:1 weird] tail]\n\nbody";
        assert!(
            Envelope::strip_from_content(content).is_none(),
            "malformed header (bracket not followed by newline) must not strip"
        );
    }

    #[test]
    fn strip_rejects_empty_content() {
        assert!(Envelope::strip_from_content("").is_none());
    }

    #[test]
    fn strip_rejects_header_not_at_start() {
        // Header anywhere other than position 0 must not match (anchored).
        let content = "preamble\n[TELEGRAM id:1 Mon 2026-01-01]\n\nbody";
        assert!(Envelope::strip_from_content(content).is_none());
    }

    #[test]
    fn strip_preserves_utf8_body() {
        let content = "[TELEGRAM id:1 Mon 2026-01-01]\n\n你好 🐾 café";
        let r = Envelope::strip_from_content(content).expect("should match");
        assert_eq!(r.stripped_content, "你好 🐾 café");
    }

    #[test]
    fn strip_preserves_body_with_leading_whitespace() {
        let content = "[TELEGRAM id:1 Mon 2026-01-01]\n\n    indented line";
        let r = Envelope::strip_from_content(content).expect("should match");
        assert_eq!(r.stripped_content, "    indented line");
    }

    #[test]
    fn strip_preserves_multiline_body() {
        let content = "[TELEGRAM id:1 Mon 2026-01-01]\n\nline1\nline2\n\nline3";
        let r = Envelope::strip_from_content(content).expect("should match");
        assert_eq!(r.stripped_content, "line1\nline2\n\nline3");
    }

    #[test]
    fn strip_captures_reply_block() {
        let content = "[TELEGRAM alice id:1 Mon 2026-04-20 09:00 -04:00]\nReplying to bob (@b) (msg_id:42):\n> hi there\n> second line\n\nthe actual reply";
        let r = Envelope::strip_from_content(content).expect("should match");
        let reply = r.reply_block.as_deref().expect("reply present");
        assert!(reply.starts_with("Replying to bob"));
        assert!(reply.contains("> hi there"));
        assert!(reply.contains("> second line"));
        assert_eq!(r.stripped_content, "the actual reply");
    }

    #[test]
    fn strip_no_reply_when_block_malformed() {
        // "Replying to" line without the `> ` quoted body — the reply block
        // is malformed. Conservative behaviour: refuse to strip anything
        // rather than risk splitting content at the wrong place.
        let content = "[TELEGRAM id:1 Mon 2026-04-20]\nReplying to bob:\nno quote marker\n\nbody";
        assert!(
            Envelope::strip_from_content(content).is_none(),
            "malformed reply block must cause full strip rejection"
        );
    }

    #[test]
    fn strip_roundtrip_from_render_for_prompt() {
        // Render a realistic envelope, then strip. The stripped result must
        // leave no header residue.
        let env = Envelope {
            sender_id: Some("7539582820".to_string()),
            sender_name: Some("potato".to_string()),
            sender_username: Some("potatosoupup".to_string()),
            chat_type: ChatType::Direct,
            reply_to: None,
            message_id: Some(1),
        };
        let prefix = env.render_for_prompt("telegram");
        let full = format!("{}actual user message", prefix);
        let r = Envelope::strip_from_content(&full).expect("rendered header must strip");
        assert_eq!(r.channel, "TELEGRAM");
        assert_eq!(r.stripped_content, "actual user message");
    }

    #[test]
    fn strip_roundtrip_with_reply() {
        let env = Envelope {
            sender_id: Some("1".to_string()),
            sender_name: Some("alice".to_string()),
            sender_username: None,
            chat_type: ChatType::Direct,
            reply_to: Some(QuotedMessage {
                text: "original question".to_string(),
                sender_name: Some("bob".to_string()),
                sender_username: Some("bobby".to_string()),
                sender_id: None,
                message_id: Some(42),
            }),
            message_id: Some(2),
        };
        let prefix = env.render_for_prompt("telegram");
        let full = format!("{}my answer", prefix);
        let r = Envelope::strip_from_content(&full).expect("should match");
        assert!(
            r.reply_block.is_some(),
            "reply block must be captured, got content={:?}",
            r.stripped_content
        );
        assert_eq!(r.stripped_content, "my answer");
    }

    #[test]
    fn strip_idempotent_on_stripped_content() {
        // Running strip twice: second call must return None (no header left).
        let content = "[TELEGRAM id:1 Mon 2026-01-01]\n\nbody";
        let first = Envelope::strip_from_content(content).expect("first pass matches");
        assert!(
            Envelope::strip_from_content(&first.stripped_content).is_none(),
            "second pass must find nothing"
        );
    }

    #[test]
    fn strip_rejects_just_header_no_body() {
        // Header with no following blank line — regex requires \n\n.
        let content = "[TELEGRAM id:1 Mon 2026-01-01]";
        assert!(Envelope::strip_from_content(content).is_none());
    }

    #[test]
    fn strip_allows_empty_body_after_header() {
        // Header + \n\n + empty body — legitimate edge case.
        let content = "[TELEGRAM id:1 Mon 2026-01-01]\n\n";
        let r = Envelope::strip_from_content(content).expect("should match");
        assert_eq!(r.stripped_content, "");
    }

    #[test]
    fn known_channels_covers_all_supported_platforms() {
        // Regression guard: adding a new channel requires updating this test.
        assert_eq!(
            KNOWN_CHANNELS,
            &["TELEGRAM", "DISCORD", "SLACK", "SIGNAL", "MATRIX", "WHATSAPP"]
        );
    }
}
