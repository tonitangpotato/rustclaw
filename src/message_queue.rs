//! Message queue for handling new messages while agent is busy.
//!
//! Allows users to send messages during tool loops. Messages are queued
//! and injected into the session before the next LLM call.

use std::time::Instant;
use tokio::sync::Mutex;

/// Priority levels for queued messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Process in current turn (interrupt-style, highest priority).
    Now = 3,
    /// Process after current tool batch completes.
    Next = 2,
    /// Process after current agent loop finishes.
    Later = 1,
}

/// A queued message waiting to be injected into the session.
#[derive(Debug, Clone)]
pub struct QueuedMessage {
    pub text: String,
    pub priority: Priority,
    pub timestamp: Instant,
    /// Optional: user who sent the message (for logging/context).
    pub user_id: Option<String>,
}

impl QueuedMessage {
    pub fn new(text: String, priority: Priority) -> Self {
        Self {
            text,
            priority,
            timestamp: Instant::now(),
            user_id: None,
        }
    }

    pub fn with_user(mut self, user_id: Option<String>) -> Self {
        self.user_id = user_id;
        self
    }
}

/// Message queue for a single session.
///
/// Thread-safe: wrapped in Arc<Mutex<>> when stored on AgentRunner.
#[derive(Debug)]
pub struct MessageQueue {
    pending: Vec<QueuedMessage>,
}

impl MessageQueue {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    /// Push a new message into the queue.
    pub fn push(&mut self, msg: QueuedMessage) {
        self.pending.push(msg);
        // Keep sorted by priority (highest first)
        self.pending.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    /// Drain all pending messages (returns them in priority order, clears queue).
    pub fn drain(&mut self) -> Vec<QueuedMessage> {
        std::mem::take(&mut self.pending)
    }

    /// Peek at highest-priority message without removing.
    pub fn peek(&self) -> Option<&QueuedMessage> {
        self.pending.first()
    }

    /// Check if queue is empty.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Number of pending messages.
    pub fn len(&self) -> usize {
        self.pending.len()
    }
}

impl Default for MessageQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-session queue manager (stored on AgentRunner).
#[derive(Debug, Clone)]
pub struct SessionQueues {
    /// session_key -> queue
    queues: std::sync::Arc<Mutex<std::collections::HashMap<String, MessageQueue>>>,
}

impl SessionQueues {
    pub fn new() -> Self {
        Self {
            queues: std::sync::Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Push a message to a session's queue.
    pub async fn push(&self, session_key: &str, msg: QueuedMessage) {
        let priority = msg.priority; // Capture before move
        let mut queues = self.queues.lock().await;
        queues.entry(session_key.to_string()).or_insert_with(MessageQueue::new).push(msg);
        tracing::debug!(
            "Queued message for session {} (priority: {:?}, queue len: {})",
            session_key,
            priority,
            queues.get(session_key).map(|q| q.len()).unwrap_or(0)
        );
    }

    /// Drain all pending messages for a session.
    pub async fn drain(&self, session_key: &str) -> Vec<QueuedMessage> {
        let mut queues = self.queues.lock().await;
        queues.get_mut(session_key).map(|q| q.drain()).unwrap_or_default()
    }

    /// Check if a session has pending messages.
    pub async fn has_pending(&self, session_key: &str) -> bool {
        let queues = self.queues.lock().await;
        queues.get(session_key).map(|q| !q.is_empty()).unwrap_or(false)
    }
}

impl Default for SessionQueues {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_ordering() {
        let mut queue = MessageQueue::new();
        queue.push(QueuedMessage::new("later".to_string(), Priority::Later));
        queue.push(QueuedMessage::new("now".to_string(), Priority::Now));
        queue.push(QueuedMessage::new("next".to_string(), Priority::Next));

        let msgs = queue.drain();
        assert_eq!(msgs[0].text, "now");
        assert_eq!(msgs[1].text, "next");
        assert_eq!(msgs[2].text, "later");
    }

    #[tokio::test]
    async fn test_session_queues() {
        let queues = SessionQueues::new();
        queues.push("sess1", QueuedMessage::new("msg1".to_string(), Priority::Next)).await;
        queues.push("sess1", QueuedMessage::new("msg2".to_string(), Priority::Now)).await;

        assert!(queues.has_pending("sess1").await);
        assert!(!queues.has_pending("sess2").await);

        let msgs = queues.drain("sess1").await;
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].text, "msg2"); // Now comes first
    }
}
