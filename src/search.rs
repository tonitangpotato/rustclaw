//! FTS5-based full-text search for session messages.
//!
//! Provides efficient search across conversation histories using SQLite FTS5
//! with BM25 ranking and highlighted snippets.

use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::Row;

/// Full-text search engine for session messages.
pub struct SessionSearch {
    db: SqlitePool,
}

/// A single search result from the FTS5 index.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Session key the message belongs to.
    pub session_key: String,
    /// Role of the message sender (user, assistant, system).
    pub role: String,
    /// Full message content.
    pub content: String,
    /// Unix timestamp of the message.
    pub timestamp: i64,
    /// BM25 relevance score (lower is more relevant).
    pub rank: f64,
    /// Highlighted snippet with search terms marked.
    pub snippet: String,
}

impl SessionSearch {
    /// Create a new SessionSearch instance, initializing the FTS5 table if needed.
    ///
    /// # Arguments
    /// * `db_path` - Path to the SQLite database file (will be created if doesn't exist)
    ///
    /// # Example
    /// ```ignore
    /// let search = SessionSearch::new("./search.db").await?;
    /// ```
    pub async fn new(db_path: &str) -> anyhow::Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&format!("sqlite:{}?mode=rwc", db_path))
            .await?;

        // Create FTS5 virtual table with porter stemming and unicode support
        sqlx::query(
            "CREATE VIRTUAL TABLE IF NOT EXISTS message_fts USING fts5(
                session_key,
                role,
                content,
                timestamp UNINDEXED,
                tokenize='porter unicode61'
            )",
        )
        .execute(&pool)
        .await?;

        tracing::info!("SessionSearch initialized with FTS5 at {}", db_path);

        Ok(Self { db: pool })
    }

    /// Create a SessionSearch from an existing pool (useful for sharing connections).
    pub async fn from_pool(pool: SqlitePool) -> anyhow::Result<Self> {
        // Ensure FTS5 table exists
        sqlx::query(
            "CREATE VIRTUAL TABLE IF NOT EXISTS message_fts USING fts5(
                session_key,
                role,
                content,
                timestamp UNINDEXED,
                tokenize='porter unicode61'
            )",
        )
        .execute(&pool)
        .await?;

        Ok(Self { db: pool })
    }

    /// Index a single message into the FTS5 table.
    ///
    /// # Arguments
    /// * `session_key` - Unique session identifier
    /// * `role` - Message role (user, assistant, system)
    /// * `content` - Message content to index
    /// * `timestamp` - Unix timestamp of the message
    pub async fn index_message(
        &self,
        session_key: &str,
        role: &str,
        content: &str,
        timestamp: i64,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO message_fts (session_key, role, content, timestamp) VALUES (?, ?, ?, ?)",
        )
        .bind(session_key)
        .bind(role)
        .bind(content)
        .bind(timestamp)
        .execute(&self.db)
        .await?;

        tracing::debug!(
            "Indexed message: session={}, role={}, len={}",
            session_key,
            role,
            content.len()
        );

        Ok(())
    }

    /// Search across all sessions using FTS5 with BM25 ranking.
    ///
    /// # Arguments
    /// * `query` - Search query (supports FTS5 syntax: AND, OR, NOT, phrases)
    /// * `limit` - Maximum number of results to return
    ///
    /// # Returns
    /// Vector of search results ordered by relevance (best matches first).
    pub async fn search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<SearchResult>> {
        // Escape query for FTS5 (wrap in quotes for phrase search if contains spaces)
        let fts_query = Self::prepare_fts_query(query);

        let rows = sqlx::query(
            "SELECT 
                session_key,
                role,
                content,
                timestamp,
                bm25(message_fts) as rank,
                snippet(message_fts, 2, '<mark>', '</mark>', '...', 64) as snippet
             FROM message_fts 
             WHERE message_fts MATCH ?
             ORDER BY rank
             LIMIT ?",
        )
        .bind(&fts_query)
        .bind(limit as i64)
        .fetch_all(&self.db)
        .await?;

        let results = rows
            .into_iter()
            .map(|row| SearchResult {
                session_key: row.get("session_key"),
                role: row.get("role"),
                content: row.get("content"),
                timestamp: row.get("timestamp"),
                rank: row.get("rank"),
                snippet: row.get("snippet"),
            })
            .collect();

        Ok(results)
    }

    /// Search within a specific session.
    ///
    /// # Arguments
    /// * `session_key` - Session to search within
    /// * `query` - Search query
    /// * `limit` - Maximum results
    pub async fn search_in_session(
        &self,
        session_key: &str,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<SearchResult>> {
        let fts_query = Self::prepare_fts_query(query);

        // Combine session filter with content query
        let combined_query = format!("session_key:\"{}\" AND ({})", session_key, fts_query);

        let rows = sqlx::query(
            "SELECT 
                session_key,
                role,
                content,
                timestamp,
                bm25(message_fts) as rank,
                snippet(message_fts, 2, '<mark>', '</mark>', '...', 64) as snippet
             FROM message_fts 
             WHERE message_fts MATCH ?
             ORDER BY rank
             LIMIT ?",
        )
        .bind(&combined_query)
        .bind(limit as i64)
        .fetch_all(&self.db)
        .await?;

        let results = rows
            .into_iter()
            .map(|row| SearchResult {
                session_key: row.get("session_key"),
                role: row.get("role"),
                content: row.get("content"),
                timestamp: row.get("timestamp"),
                rank: row.get("rank"),
                snippet: row.get("snippet"),
            })
            .collect();

        Ok(results)
    }

    /// Format search results for LLM consumption.
    ///
    /// Produces a human-readable summary suitable for including in prompts.
    pub async fn summarize_results(&self, results: &[SearchResult]) -> String {
        if results.is_empty() {
            return "No matching messages found.".to_string();
        }

        let mut summary = format!("Found {} relevant messages:\n\n", results.len());

        for (i, result) in results.iter().enumerate() {
            let role_display = match result.role.as_str() {
                "user" => "User",
                "assistant" => "Assistant",
                "system" => "System",
                other => other,
            };

            summary.push_str(&format!(
                "{}. [Session: {}] {} (relevance: {:.2}):\n   {}\n\n",
                i + 1,
                result.session_key,
                role_display,
                -result.rank, // BM25 returns negative scores, negate for display
                result.snippet
            ));
        }

        summary
    }

    /// Reindex all messages from the session database.
    ///
    /// This is useful for rebuilding the search index from scratch.
    ///
    /// # Arguments
    /// * `session_db` - Pool connected to the sessions database
    ///
    /// # Returns
    /// Number of messages indexed.
    pub async fn reindex_all(&self, session_db: &SqlitePool) -> anyhow::Result<usize> {
        // Clear existing index
        sqlx::query("DELETE FROM message_fts")
            .execute(&self.db)
            .await?;

        tracing::info!("Cleared FTS5 index, starting reindex...");

        // Fetch all sessions
        let sessions: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT key, messages, updated_at FROM sessions",
        )
        .fetch_all(session_db)
        .await?;

        let mut total_indexed = 0;

        for (session_key, messages_json, updated_at) in sessions {
            // Parse messages JSON
            let messages: Vec<serde_json::Value> =
                serde_json::from_str(&messages_json).unwrap_or_default();

            // Parse timestamp from updated_at (RFC3339)
            let base_timestamp = chrono::DateTime::parse_from_rfc3339(&updated_at)
                .map(|dt| dt.timestamp())
                .unwrap_or(0);

            for (idx, msg) in messages.iter().enumerate() {
                let role = msg["role"].as_str().unwrap_or("unknown");

                // Extract text content from content blocks
                let content = Self::extract_message_content(msg);

                if content.is_empty() {
                    continue;
                }

                // Use base timestamp + index as approximate message time
                let timestamp = base_timestamp - (messages.len() - idx) as i64;

                self.index_message(&session_key, role, &content, timestamp)
                    .await?;
                total_indexed += 1;
            }
        }

        tracing::info!("Reindexed {} messages", total_indexed);

        Ok(total_indexed)
    }

    /// Prepare a query string for FTS5 matching.
    fn prepare_fts_query(query: &str) -> String {
        let trimmed = query.trim();

        // If query contains FTS5 operators, use as-is
        if trimmed.contains(" AND ")
            || trimmed.contains(" OR ")
            || trimmed.contains(" NOT ")
            || trimmed.starts_with('"')
        {
            return trimmed.to_string();
        }

        // For simple queries, escape special characters and wrap terms
        let words: Vec<&str> = trimmed.split_whitespace().collect();
        if words.len() == 1 {
            // Single word: use prefix matching
            format!("{}*", escape_fts_term(words[0]))
        } else {
            // Multiple words: search for all terms
            words
                .iter()
                .map(|w| escape_fts_term(w))
                .collect::<Vec<_>>()
                .join(" ")
        }
    }

    /// Extract text content from a message JSON value.
    fn extract_message_content(msg: &serde_json::Value) -> String {
        // Handle content as array of blocks (Anthropic format)
        if let Some(content_arr) = msg["content"].as_array() {
            return content_arr
                .iter()
                .filter_map(|block| {
                    if let Some(text) = block["text"].as_str() {
                        Some(text.to_string())
                    } else if let Some(name) = block["name"].as_str() {
                        // Tool use
                        Some(format!("[Tool: {}]", name))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
        }

        // Handle content as simple string
        if let Some(text) = msg["content"].as_str() {
            return text.to_string();
        }

        String::new()
    }

    /// Get the underlying database pool.
    pub fn pool(&self) -> &SqlitePool {
        &self.db
    }
}

/// Escape special FTS5 characters in a search term.
fn escape_fts_term(term: &str) -> String {
    // FTS5 special chars: " * ^ -
    // Wrap in quotes if contains special chars
    if term.contains('"') || term.contains('*') || term.contains('^') || term.contains('-') {
        format!("\"{}\"", term.replace('"', "\"\""))
    } else {
        term.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_session_search_new() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test_search.db");

        let search = SessionSearch::new(db_path.to_str().unwrap()).await;
        assert!(search.is_ok());
    }

    #[tokio::test]
    async fn test_index_and_search() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test_search.db");

        let search = SessionSearch::new(db_path.to_str().unwrap())
            .await
            .unwrap();

        // Index some messages
        search
            .index_message("session1", "user", "Hello, how are you?", 1000)
            .await
            .unwrap();
        search
            .index_message("session1", "assistant", "I'm doing great, thanks!", 1001)
            .await
            .unwrap();
        search
            .index_message("session2", "user", "What is the weather today?", 2000)
            .await
            .unwrap();

        // Search for "great"
        let results = search.search("great", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].role, "assistant");
        assert!(results[0].content.contains("great"));

        // Search for "hello"
        let results = search.search("hello", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].session_key, "session1");
    }

    #[tokio::test]
    async fn test_search_in_session() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test_search.db");

        let search = SessionSearch::new(db_path.to_str().unwrap())
            .await
            .unwrap();

        // Index messages in different sessions
        search
            .index_message("session1", "user", "I love Rust programming", 1000)
            .await
            .unwrap();
        search
            .index_message("session2", "user", "I love Python programming", 2000)
            .await
            .unwrap();

        // Search in session1 only
        let results = search.search_in_session("session1", "programming", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("Rust"));
    }

    #[tokio::test]
    async fn test_summarize_results() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test_search.db");

        let search = SessionSearch::new(db_path.to_str().unwrap())
            .await
            .unwrap();

        let results = vec![
            SearchResult {
                session_key: "sess1".to_string(),
                role: "user".to_string(),
                content: "Test content".to_string(),
                timestamp: 1000,
                rank: -5.0,
                snippet: "Test <mark>content</mark>".to_string(),
            },
        ];

        let summary = search.summarize_results(&results).await;
        assert!(summary.contains("Found 1 relevant"));
        assert!(summary.contains("sess1"));
        assert!(summary.contains("User"));
    }

    #[test]
    fn test_prepare_fts_query() {
        // Single word gets prefix matching
        assert_eq!(SessionSearch::prepare_fts_query("hello"), "hello*");

        // Multiple words
        assert_eq!(
            SessionSearch::prepare_fts_query("hello world"),
            "hello world"
        );

        // FTS5 operators preserved
        assert_eq!(
            SessionSearch::prepare_fts_query("hello AND world"),
            "hello AND world"
        );

        // Quoted phrases preserved
        assert_eq!(
            SessionSearch::prepare_fts_query("\"exact phrase\""),
            "\"exact phrase\""
        );
    }

    #[test]
    fn test_escape_fts_term() {
        assert_eq!(escape_fts_term("hello"), "hello");
        assert_eq!(escape_fts_term("hello-world"), "\"hello-world\"");
        assert_eq!(escape_fts_term("test*"), "\"test*\"");
    }

    #[test]
    fn test_extract_message_content() {
        // Simple string content
        let msg1 = serde_json::json!({
            "role": "user",
            "content": "Hello world"
        });
        assert_eq!(
            SessionSearch::extract_message_content(&msg1),
            "Hello world"
        );

        // Array content (Anthropic format)
        let msg2 = serde_json::json!({
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Hello"},
                {"type": "text", "text": "World"}
            ]
        });
        assert_eq!(
            SessionSearch::extract_message_content(&msg2),
            "Hello World"
        );

        // Tool use content
        let msg3 = serde_json::json!({
            "role": "assistant",
            "content": [
                {"type": "tool_use", "name": "exec", "input": {}}
            ]
        });
        assert_eq!(
            SessionSearch::extract_message_content(&msg3),
            "[Tool: exec]"
        );
    }
}
