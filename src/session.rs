//! Session management with in-memory storage (SQLite persistence TODO).

use chrono::Utc;
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
}

/// Manages all active sessions.
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<String, Session>>>,
}

impl SessionManager {
    pub async fn new(_config: &Config) -> anyhow::Result<Self> {
        Ok(Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Get or create a session.
    pub async fn get_or_create(&self, key: &str) -> Session {
        let mut sessions = self.sessions.write().await;
        sessions
            .entry(key.to_string())
            .or_insert_with(|| Session::new(key))
            .clone()
    }

    /// Update a session.
    pub async fn update(&self, session: Session) {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session.key.clone(), session);
    }
}
