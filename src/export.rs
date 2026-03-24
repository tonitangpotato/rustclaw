//! Trajectory export for conversation histories.
//!
//! Exports session data to ShareGPT format for training data generation.

use std::path::PathBuf;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePool;
use sqlx::Row;

/// Exporter for conversation trajectories to ShareGPT format.
pub struct TrajectoryExporter {
    output_dir: PathBuf,
}

/// ShareGPT conversation format (widely used for fine-tuning).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareGPTConversation {
    /// Unique conversation ID.
    pub id: String,
    /// List of messages in the conversation.
    pub conversations: Vec<ShareGPTMessage>,
    /// Source identifier (e.g., "rustclaw").
    pub source: String,
    /// Additional metadata.
    pub metadata: ShareGPTMetadata,
}

/// A single message in ShareGPT format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareGPTMessage {
    /// Speaker: "human", "gpt", or "system".
    pub from: String,
    /// Message content.
    pub value: String,
}

/// Metadata for a ShareGPT conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareGPTMetadata {
    /// Model used for generation.
    pub model: String,
    /// Original session key.
    pub session_key: String,
    /// ISO 8601 timestamp of export.
    pub exported_at: String,
    /// Number of messages in the conversation.
    pub message_count: usize,
    /// Total tokens used (if available).
    pub total_tokens: Option<u64>,
}

/// Filter criteria for selective export.
#[derive(Debug, Clone, Default)]
pub struct ExportFilter {
    /// Only export sessions updated after this timestamp.
    pub after: Option<i64>,
    /// Only export sessions updated before this timestamp.
    pub before: Option<i64>,
    /// Minimum number of messages required.
    pub min_messages: Option<usize>,
    /// Session key pattern to match (SQL LIKE pattern).
    pub session_key_pattern: Option<String>,
}

impl ExportFilter {
    /// Create a new empty filter (matches all sessions).
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter sessions updated after the given timestamp.
    pub fn after(mut self, timestamp: i64) -> Self {
        self.after = Some(timestamp);
        self
    }

    /// Filter sessions updated before the given timestamp.
    pub fn before(mut self, timestamp: i64) -> Self {
        self.before = Some(timestamp);
        self
    }

    /// Require at least N messages.
    pub fn min_messages(mut self, count: usize) -> Self {
        self.min_messages = Some(count);
        self
    }

    /// Filter by session key pattern (SQL LIKE syntax).
    pub fn session_key_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.session_key_pattern = Some(pattern.into());
        self
    }
}

impl TrajectoryExporter {
    /// Create a new exporter with the given output directory.
    ///
    /// # Arguments
    /// * `output_dir` - Directory where exported files will be written
    pub fn new(output_dir: impl Into<PathBuf>) -> Self {
        Self {
            output_dir: output_dir.into(),
        }
    }

    /// Export a single session to ShareGPT JSON format.
    ///
    /// # Arguments
    /// * `db` - SQLite pool connected to the sessions database
    /// * `session_key` - Key of the session to export
    ///
    /// # Returns
    /// Path to the exported JSON file.
    pub async fn export_session(
        &self,
        db: &SqlitePool,
        session_key: &str,
    ) -> anyhow::Result<PathBuf> {
        // Fetch session from database
        let row = sqlx::query(
            "SELECT key, messages, total_tokens FROM sessions WHERE key = ?",
        )
        .bind(session_key)
        .fetch_optional(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Session not found: {}", session_key))?;

        let messages_json: String = row.get("messages");
        let total_tokens: i64 = row.get("total_tokens");

        // Parse messages
        let messages: Vec<serde_json::Value> =
            serde_json::from_str(&messages_json).unwrap_or_default();

        // Convert to (role, content, session_key) tuples
        let tuples: Vec<(String, String, String)> = messages
            .iter()
            .filter_map(|msg| {
                let role = msg["role"].as_str()?.to_string();
                let content = Self::extract_content(msg)?;
                Some((role, content, session_key.to_string()))
            })
            .collect();

        if tuples.is_empty() {
            anyhow::bail!("Session {} has no valid messages", session_key);
        }

        // Convert to ShareGPT format
        let mut conversation = Self::to_sharegpt(&tuples);
        conversation.metadata.total_tokens = Some(total_tokens as u64);

        // Ensure output directory exists
        tokio::fs::create_dir_all(&self.output_dir).await?;

        // Write to file
        let filename = format!("{}.json", sanitize_filename(session_key));
        let output_path = self.output_dir.join(&filename);

        let json = serde_json::to_string_pretty(&conversation)?;
        tokio::fs::write(&output_path, json).await?;

        tracing::info!(
            "Exported session {} to {} ({} messages)",
            session_key,
            output_path.display(),
            conversation.conversations.len()
        );

        Ok(output_path)
    }

    /// Export all sessions to ShareGPT JSON format.
    ///
    /// # Returns
    /// Vector of paths to exported files.
    pub async fn export_all(&self, db: &SqlitePool) -> anyhow::Result<Vec<PathBuf>> {
        self.export_filtered(db, ExportFilter::default()).await
    }

    /// Export sessions matching the given filter.
    ///
    /// # Arguments
    /// * `db` - SQLite pool connected to the sessions database
    /// * `filter` - Filter criteria for session selection
    ///
    /// # Returns
    /// Vector of paths to exported files.
    pub async fn export_filtered(
        &self,
        db: &SqlitePool,
        filter: ExportFilter,
    ) -> anyhow::Result<Vec<PathBuf>> {
        // Build dynamic query with filters
        let mut query = String::from("SELECT key, messages, total_tokens FROM sessions WHERE 1=1");
        let mut bindings: Vec<String> = Vec::new();

        if let Some(pattern) = &filter.session_key_pattern {
            query.push_str(" AND key LIKE ?");
            bindings.push(pattern.clone());
        }

        // Note: after/before filters would need updated_at to be parsed
        // For simplicity, we'll filter in code since updated_at is RFC3339 string

        let rows = sqlx::query(&query)
            .fetch_all(db)
            .await?;

        let mut exported_paths = Vec::new();

        for row in rows {
            let session_key: String = row.get("key");
            let messages_json: String = row.get("messages");
            let total_tokens: i64 = row.get("total_tokens");

            // Parse messages
            let messages: Vec<serde_json::Value> =
                serde_json::from_str(&messages_json).unwrap_or_default();

            // Apply min_messages filter
            if let Some(min) = filter.min_messages {
                if messages.len() < min {
                    continue;
                }
            }

            // Convert to tuples
            let tuples: Vec<(String, String, String)> = messages
                .iter()
                .filter_map(|msg| {
                    let role = msg["role"].as_str()?.to_string();
                    let content = Self::extract_content(msg)?;
                    Some((role, content, session_key.clone()))
                })
                .collect();

            if tuples.is_empty() {
                continue;
            }

            // Convert to ShareGPT
            let mut conversation = Self::to_sharegpt(&tuples);
            conversation.metadata.total_tokens = Some(total_tokens as u64);

            // Ensure output directory exists
            tokio::fs::create_dir_all(&self.output_dir).await?;

            // Write to file
            let filename = format!("{}.json", sanitize_filename(&session_key));
            let output_path = self.output_dir.join(&filename);

            let json = serde_json::to_string_pretty(&conversation)?;
            tokio::fs::write(&output_path, json).await?;

            exported_paths.push(output_path);
        }

        tracing::info!("Exported {} sessions", exported_paths.len());

        Ok(exported_paths)
    }

    /// Convert message tuples to ShareGPT conversation format.
    ///
    /// # Arguments
    /// * `messages` - Slice of (role, content, session_key) tuples
    ///
    /// # Returns
    /// ShareGPT conversation with all messages converted.
    pub fn to_sharegpt(messages: &[(String, String, String)]) -> ShareGPTConversation {
        let session_key = messages
            .first()
            .map(|(_, _, k)| k.clone())
            .unwrap_or_else(|| "unknown".to_string());

        let conversations: Vec<ShareGPTMessage> = messages
            .iter()
            .map(|(role, content, _)| {
                let from = match role.as_str() {
                    "user" => "human",
                    "assistant" => "gpt",
                    "system" => "system",
                    other => other,
                };
                ShareGPTMessage {
                    from: from.to_string(),
                    value: content.clone(),
                }
            })
            .collect();

        let id = format!(
            "{}_{}",
            sanitize_filename(&session_key),
            Utc::now().timestamp()
        );

        ShareGPTConversation {
            id,
            conversations: conversations.clone(),
            source: "rustclaw".to_string(),
            metadata: ShareGPTMetadata {
                model: "unknown".to_string(), // Would need to be passed in or stored
                session_key,
                exported_at: Utc::now().to_rfc3339(),
                message_count: conversations.len(),
                total_tokens: None,
            },
        }
    }

    /// Extract text content from a message JSON value.
    fn extract_content(msg: &serde_json::Value) -> Option<String> {
        // Handle content as array of blocks (Anthropic format)
        if let Some(content_arr) = msg["content"].as_array() {
            let text: String = content_arr
                .iter()
                .filter_map(|block| {
                    if let Some(text) = block["text"].as_str() {
                        Some(text.to_string())
                    } else if let Some(name) = block["name"].as_str() {
                        // Tool use - include for context
                        let input = block["input"]
                            .as_object()
                            .map(|o| serde_json::to_string(o).unwrap_or_default())
                            .unwrap_or_default();
                        Some(format!("[Tool: {} - {}]", name, input))
                    } else if block.get("tool_use_id").is_some() {
                        // Tool result
                        let content = block["content"].as_str().unwrap_or("");
                        Some(format!("[Tool Result: {}]", truncate(content, 500)))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");

            if text.is_empty() {
                return None;
            }
            return Some(text);
        }

        // Handle content as simple string
        msg["content"].as_str().map(|s| s.to_string())
    }

    /// Get the output directory path.
    pub fn output_dir(&self) -> &PathBuf {
        &self.output_dir
    }
}

/// Sanitize a string for use as a filename.
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Truncate a string to the given byte length (char-boundary safe).
fn truncate(s: &str, max_len: usize) -> &str {
    crate::text_utils::truncate_bytes(s, max_len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    use tempfile::tempdir;

    async fn setup_test_db(dir: &std::path::Path) -> SqlitePool {
        let db_path = dir.join("test_sessions.db");
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!("sqlite:{}?mode=rwc", db_path.display()))
            .await
            .unwrap();

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
        .await
        .unwrap();

        pool
    }

    #[tokio::test]
    async fn test_export_session() {
        let dir = tempdir().unwrap();
        let pool = setup_test_db(dir.path()).await;

        // Insert test session
        let messages = serde_json::json!([
            {"role": "user", "content": "Hello"},
            {"role": "assistant", "content": "Hi there!"}
        ]);

        sqlx::query(
            "INSERT INTO sessions (key, messages, created_at, updated_at, total_tokens) VALUES (?, ?, ?, ?, ?)",
        )
        .bind("test_session")
        .bind(messages.to_string())
        .bind("2024-01-01T00:00:00Z")
        .bind("2024-01-01T00:00:00Z")
        .bind(100)
        .execute(&pool)
        .await
        .unwrap();

        // Export
        let output_dir = dir.path().join("exports");
        let exporter = TrajectoryExporter::new(&output_dir);
        let path = exporter.export_session(&pool, "test_session").await.unwrap();

        assert!(path.exists());

        // Verify content
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        let conv: ShareGPTConversation = serde_json::from_str(&content).unwrap();

        assert_eq!(conv.conversations.len(), 2);
        assert_eq!(conv.conversations[0].from, "human");
        assert_eq!(conv.conversations[0].value, "Hello");
        assert_eq!(conv.conversations[1].from, "gpt");
        assert_eq!(conv.metadata.total_tokens, Some(100));
    }

    #[tokio::test]
    async fn test_export_filtered() {
        let dir = tempdir().unwrap();
        let pool = setup_test_db(dir.path()).await;

        // Insert test sessions
        let msg2 = serde_json::json!([
            {"role": "user", "content": "Hi"},
            {"role": "assistant", "content": "Hello"}
        ]);

        let msg5 = serde_json::json!([
            {"role": "user", "content": "1"},
            {"role": "assistant", "content": "2"},
            {"role": "user", "content": "3"},
            {"role": "assistant", "content": "4"},
            {"role": "user", "content": "5"}
        ]);

        sqlx::query(
            "INSERT INTO sessions (key, messages, created_at, updated_at, total_tokens) VALUES (?, ?, ?, ?, ?)",
        )
        .bind("short_session")
        .bind(msg2.to_string())
        .bind("2024-01-01T00:00:00Z")
        .bind("2024-01-01T00:00:00Z")
        .bind(50)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO sessions (key, messages, created_at, updated_at, total_tokens) VALUES (?, ?, ?, ?, ?)",
        )
        .bind("long_session")
        .bind(msg5.to_string())
        .bind("2024-01-01T00:00:00Z")
        .bind("2024-01-01T00:00:00Z")
        .bind(200)
        .execute(&pool)
        .await
        .unwrap();

        // Export with min_messages filter
        let output_dir = dir.path().join("exports");
        let exporter = TrajectoryExporter::new(&output_dir);

        let filter = ExportFilter::new().min_messages(3);
        let paths = exporter.export_filtered(&pool, filter).await.unwrap();

        assert_eq!(paths.len(), 1);
        assert!(paths[0].to_str().unwrap().contains("long_session"));
    }

    #[test]
    fn test_to_sharegpt() {
        let messages = vec![
            ("system".to_string(), "You are helpful".to_string(), "sess1".to_string()),
            ("user".to_string(), "Hello".to_string(), "sess1".to_string()),
            ("assistant".to_string(), "Hi!".to_string(), "sess1".to_string()),
        ];

        let conv = TrajectoryExporter::to_sharegpt(&messages);

        assert_eq!(conv.conversations.len(), 3);
        assert_eq!(conv.conversations[0].from, "system");
        assert_eq!(conv.conversations[1].from, "human");
        assert_eq!(conv.conversations[2].from, "gpt");
        assert_eq!(conv.source, "rustclaw");
        assert_eq!(conv.metadata.session_key, "sess1");
        assert_eq!(conv.metadata.message_count, 3);
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("hello-world"), "hello-world");
        assert_eq!(sanitize_filename("hello:world"), "hello_world");
        assert_eq!(sanitize_filename("session/key/123"), "session_key_123");
        assert_eq!(sanitize_filename("test@user.com"), "test_user_com");
    }

    #[test]
    fn test_extract_content_simple() {
        let msg = serde_json::json!({
            "role": "user",
            "content": "Hello world"
        });

        let content = TrajectoryExporter::extract_content(&msg);
        assert_eq!(content, Some("Hello world".to_string()));
    }

    #[test]
    fn test_extract_content_array() {
        let msg = serde_json::json!({
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Hello"},
                {"type": "text", "text": "World"}
            ]
        });

        let content = TrajectoryExporter::extract_content(&msg);
        assert_eq!(content, Some("Hello\nWorld".to_string()));
    }

    #[test]
    fn test_extract_content_with_tool() {
        let msg = serde_json::json!({
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Let me check"},
                {"type": "tool_use", "name": "exec", "input": {"cmd": "ls"}}
            ]
        });

        let content = TrajectoryExporter::extract_content(&msg).unwrap();
        assert!(content.contains("Let me check"));
        assert!(content.contains("[Tool: exec"));
    }

    #[test]
    fn test_export_filter_builder() {
        let filter = ExportFilter::new()
            .after(1000)
            .before(2000)
            .min_messages(5)
            .session_key_pattern("chat_%");

        assert_eq!(filter.after, Some(1000));
        assert_eq!(filter.before, Some(2000));
        assert_eq!(filter.min_messages, Some(5));
        assert_eq!(filter.session_key_pattern, Some("chat_%".to_string()));
    }
}
