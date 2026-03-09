//! Session management with SQLite persistence.

use chrono::Utc;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::Config;
use crate::llm::Message;

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

        if has_system_first && self.messages.len() > 1 {
            // Keep first message + last (max_messages - 1) messages
            let keep_from_end = max_messages.saturating_sub(1);
            let start_idx = self.messages.len() - keep_from_end;
            
            let first_msg = self.messages[0].clone();
            let tail: Vec<_> = self.messages[start_idx..].to_vec();
            
            self.messages.clear();
            self.messages.push(first_msg);
            self.messages.extend(tail);
        } else {
            // No system message, just keep last N messages
            let start_idx = self.messages.len() - max_messages;
            self.messages = self.messages[start_idx..].to_vec();
        }

        tracing::debug!(
            "Trimmed session '{}' to {} messages",
            self.key,
            self.messages.len()
        );
    }
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
