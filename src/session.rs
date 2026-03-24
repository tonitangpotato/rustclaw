//! Session management with SQLite persistence.
//!
//! Includes session summarization for long conversations.

use chrono::Utc;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::Config;
use crate::llm::{ContentBlock, LlmClient, Message};

/// A conversation session.
#[derive(Debug, Clone)]
pub struct Session {
    pub key: String,
    pub messages: Vec<Message>,
    pub created_at: String,
    pub updated_at: String,
    pub total_tokens: u64,
    pub channel: Option<String>,
    pub user_id: Option<String>,
}

impl Session {
    pub fn new(key: &str) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            key: key.to_string(),
            messages: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
            total_tokens: 0,
            channel: None,
            user_id: None,
        }
    }

    /// Trim messages to stay within context window limits.
    /// Keeps the first message (system context if present) and last N messages.
    pub fn trim_messages(&mut self, max_messages: usize) {
        if self.messages.len() <= max_messages {
            return;
        }

        // If we have at least 2 messages and the first is "system" role, keep it
        let has_system_first = self.messages.first()
            .map(|m| m.role == "system")
            .unwrap_or(false);

        let raw_start = if has_system_first && self.messages.len() > 1 {
            let keep_from_end = max_messages.saturating_sub(1);
            self.messages.len() - keep_from_end
        } else {
            self.messages.len() - max_messages
        };

        // Ensure we don't split in the middle of a tool_use/tool_result pair
        let mut start_idx = self.safe_split_index(raw_start);

        // Anthropic requires first message after system to be role=user.
        // If we'd start with an assistant message, advance until we hit a user message.
        while start_idx < self.messages.len() {
            if self.messages[start_idx].role == "user" {
                // But skip user messages that are pure tool_results (they need preceding assistant)
                let is_pure_tool_result = self.messages[start_idx].content.iter().all(|b| {
                    matches!(b, ContentBlock::ToolResult { .. })
                });
                if !is_pure_tool_result {
                    break;
                }
            }
            start_idx += 1;
        }

        if has_system_first && self.messages.len() > 1 {
            let first_msg = self.messages[0].clone();
            let tail: Vec<_> = self.messages[start_idx..].to_vec();
            
            self.messages.clear();
            self.messages.push(first_msg);
            self.messages.extend(tail);
        } else {
            self.messages = self.messages[start_idx..].to_vec();
        }

        tracing::debug!(
            "Trimmed session '{}' to {} messages",
            self.key,
            self.messages.len()
        );
    }

    /// Find a safe split index that doesn't orphan tool_result blocks.
    /// Given a desired split point, walk it backwards if the message at that
    /// index is a user message containing tool_result blocks (which need the
    /// preceding assistant tool_use message).
    fn safe_split_index(&self, mut idx: usize) -> usize {
        while idx > 0 {
            let msg = &self.messages[idx];
            let has_tool_result = msg.role == "user" && msg.content.iter().any(|b| {
                matches!(b, ContentBlock::ToolResult { .. })
            });
            if has_tool_result {
                idx -= 1;
            } else {
                break;
            }
        }
        idx
    }

    /// Summarize old messages instead of just trimming.
    /// Returns the messages that were summarized (for LLM call).
    pub fn prepare_for_summarization(&self, max_messages: usize) -> Option<(Vec<Message>, usize)> {
        if self.messages.len() <= max_messages {
            return None;
        }

        // Determine how many messages to summarize
        // Keep the last (max_messages - 1) messages, plus 1 for the summary
        let keep_recent = max_messages.saturating_sub(1);
        let mut summarize_count = self.messages.len().saturating_sub(keep_recent);

        if summarize_count < 2 {
            // Not enough messages to summarize
            return None;
        }

        // Ensure we don't split in the middle of a tool_use/tool_result pair
        let safe_idx = self.safe_split_index(summarize_count);
        if safe_idx != summarize_count {
            tracing::debug!(
                "Adjusted summarization split {} -> {} to preserve tool pairing",
                summarize_count, safe_idx
            );
            summarize_count = safe_idx;
            if summarize_count < 2 {
                return None;
            }
        }

        // Get the messages to be summarized (first N messages)
        let to_summarize: Vec<Message> = self.messages[..summarize_count].to_vec();

        Some((to_summarize, summarize_count))
    }

    /// Apply a summary to the session, replacing old messages.
    pub fn apply_summary(&mut self, summary: &str, summarized_count: usize) {
        if summarized_count >= self.messages.len() {
            // Edge case: all messages were summarized
            self.messages.clear();
            self.messages.push(Message::text("system", &format!(
                "[Previous conversation summary]\n{}",
                summary
            )));
        } else {
            // Remove old messages and prepend summary
            let remaining: Vec<Message> = self.messages[summarized_count..].to_vec();
            self.messages.clear();
            self.messages.push(Message::text("system", &format!(
                "[Previous conversation summary]\n{}",
                summary
            )));
            self.messages.extend(remaining);
        }

        tracing::info!(
            "Session '{}': summarized {} messages into 1 summary",
            self.key,
            summarized_count
        );
    }
}

/// Format messages for summarization prompt.
pub fn format_messages_for_summary(messages: &[Message]) -> String {
    let mut formatted = String::new();

    for msg in messages {
        let role = match msg.role.as_str() {
            "user" => "User",
            "assistant" => "Assistant",
            "system" => "System",
            _ => &msg.role,
        };

        // Extract text content from content blocks
        let text_content: String = msg
            .content
            .iter()
            .filter_map(|block| {
                match block {
                    ContentBlock::Text { text } => Some(text.clone()),
                    ContentBlock::ToolUse { name, .. } => Some(format!("[Tool: {}]", name)),
                    ContentBlock::ToolResult { content, .. } => {
                        // Truncate long tool results
                        let truncated = crate::text_utils::truncate_chars(content, 200);
                        Some(format!("[Result: {}]", truncated))
                    }
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        formatted.push_str(&format!("{}: {}\n", role, text_content));
    }

    formatted
}

/// Manages all active sessions with SQLite backing.
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<String, Session>>>,
    pool: Option<SqlitePool>,
}

impl SessionManager {
    pub async fn new(config: &Config) -> anyhow::Result<Self> {
        // Try to open SQLite database for session persistence
        let workspace = config.workspace.as_deref().unwrap_or(".");
        let db_path = format!("{}/sessions.db", workspace);

        let pool = match SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&format!("sqlite:{}?mode=rwc", db_path))
            .await
        {
            Ok(pool) => {
                // Create tables
                sqlx::query(
                    "CREATE TABLE IF NOT EXISTS sessions (
                        key TEXT PRIMARY KEY,
                        messages TEXT NOT NULL DEFAULT '[]',
                        created_at TEXT NOT NULL,
                        updated_at TEXT NOT NULL,
                        total_tokens INTEGER NOT NULL DEFAULT 0,
                        channel TEXT,
                        user_id TEXT
                    )",
                )
                .execute(&pool)
                .await?;

                tracing::info!("Session DB initialized: {}", db_path);
                Some(pool)
            }
            Err(e) => {
                tracing::warn!("Failed to open session DB (using in-memory): {}", e);
                None
            }
        };

        Ok(Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            pool,
        })
    }

    /// Get or create a session.
    pub async fn get_or_create(&self, key: &str) -> Session {
        // Check memory cache first
        {
            let sessions = self.sessions.read().await;
            if let Some(s) = sessions.get(key) {
                return s.clone();
            }
        }

        // Try loading from DB
        if let Some(pool) = &self.pool {
            if let Ok(row) = sqlx::query_as::<_, SessionRow>(
                "SELECT key, messages, created_at, updated_at, total_tokens, channel, user_id FROM sessions WHERE key = ?",
            )
            .bind(key)
            .fetch_one(pool)
            .await
            {
                let messages: Vec<Message> =
                    serde_json::from_str(&row.messages).unwrap_or_default();
                let session = Session {
                    key: row.key,
                    messages,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                    total_tokens: row.total_tokens as u64,
                    channel: row.channel,
                    user_id: row.user_id,
                };

                // Cache it
                let mut sessions = self.sessions.write().await;
                sessions.insert(key.to_string(), session.clone());
                return session;
            }
        }

        // Create new
        let session = Session::new(key);
        let mut sessions = self.sessions.write().await;
        sessions.insert(key.to_string(), session.clone());
        session
    }

    /// Update a session (memory + DB).
    pub async fn update(&self, session: Session) {
        let key = session.key.clone();

        // Update memory cache
        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(key.clone(), session.clone());
        }

        // Persist to DB
        if let Some(pool) = &self.pool {
            let messages_json = serde_json::to_string(&session.messages).unwrap_or_default();
            let now = Utc::now().to_rfc3339();

            if let Err(e) = sqlx::query(
                "INSERT INTO sessions (key, messages, created_at, updated_at, total_tokens, channel, user_id)
                 VALUES (?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(key) DO UPDATE SET
                    messages = excluded.messages,
                    updated_at = excluded.updated_at,
                    total_tokens = excluded.total_tokens",
            )
            .bind(&key)
            .bind(&messages_json)
            .bind(&session.created_at)
            .bind(&now)
            .bind(session.total_tokens as i64)
            .bind(&session.channel)
            .bind(&session.user_id)
            .execute(pool)
            .await
            {
                tracing::error!("Failed to persist session {}: {}", key, e);
            }
        }
    }

    /// List all sessions (from DB).
    pub async fn list_sessions(&self) -> Vec<Session> {
        if let Some(pool) = &self.pool {
            match sqlx::query_as::<_, SessionRow>(
                "SELECT key, messages, created_at, updated_at, total_tokens, channel, user_id
                 FROM sessions ORDER BY updated_at DESC LIMIT 100",
            )
            .fetch_all(pool)
            .await
            {
                Ok(rows) => rows
                    .into_iter()
                    .map(|row| {
                        let messages: Vec<Message> =
                            serde_json::from_str(&row.messages).unwrap_or_default();
                        Session {
                            key: row.key,
                            messages,
                            created_at: row.created_at,
                            updated_at: row.updated_at,
                            total_tokens: row.total_tokens as u64,
                            channel: row.channel,
                            user_id: row.user_id,
                        }
                    })
                    .collect(),
                Err(e) => {
                    tracing::error!("Failed to list sessions: {}", e);
                    vec![]
                }
            }
        } else {
            // Return in-memory sessions
            let sessions = self.sessions.read().await;
            sessions.values().cloned().collect()
        }
    }

    /// Count active sessions.
    pub async fn count(&self) -> usize {
        if let Some(pool) = &self.pool {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sessions")
                .fetch_one(pool)
                .await
                .unwrap_or(0) as usize
        } else {
            self.sessions.read().await.len()
        }
    }
}

/// Summarize old messages using the LLM.
pub async fn summarize_old_messages(
    session: &mut Session,
    max_messages: usize,
    llm: &dyn LlmClient,
) -> anyhow::Result<bool> {
    // Check if summarization is needed
    let (messages_to_summarize, count) = match session.prepare_for_summarization(max_messages) {
        Some(data) => data,
        None => return Ok(false), // No summarization needed
    };

    // Format messages for the summary prompt
    let conversation_text = format_messages_for_summary(&messages_to_summarize);

    let summary_prompt = format!(
        "Summarize the following conversation in a single concise paragraph. \
         Focus on key topics discussed, decisions made, and important context. \
         Do not include greetings or meta-commentary.\n\n\
         CONVERSATION:\n{}\n\n\
         SUMMARY:",
        conversation_text
    );

    // Call LLM to generate summary
    let response = llm
        .chat(
            "You are a helpful assistant that summarizes conversations concisely.",
            &[Message::text("user", &summary_prompt)],
            &[], // No tools needed
        )
        .await?;

    let summary = response.text.unwrap_or_else(|| {
        "[Summary unavailable]".to_string()
    });

    // Apply the summary to the session
    session.apply_summary(&summary, count);

    Ok(true)
}

/// SQLite row mapping.
#[derive(sqlx::FromRow)]
struct SessionRow {
    key: String,
    messages: String,
    created_at: String,
    updated_at: String,
    total_tokens: i64,
    channel: Option<String>,
    user_id: Option<String>,
}
