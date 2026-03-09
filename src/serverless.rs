//! Serverless/Modal Execution Backend.
//!
//! Hibernate agent state and resume on demand for cost-efficient deployment.
//! Inspired by Hermes Agent's Modal backend — environment hibernates when idle,
//! wakes on demand with near-zero cost between sessions.
//!
//! Features:
//! - Hibernate agent state to disk
//! - Resume from hibernation on demand
//! - HTTP wake server for external triggers
//! - Automatic cleanup of old states
//! - Idle timeout detection

use std::path::PathBuf;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::io::AsyncWriteExt;

/// Serverless runtime for agent hibernation/resumption.
pub struct ServerlessRuntime {
    state_dir: PathBuf,
    config: ServerlessConfig,
}

/// Configuration for the serverless runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerlessConfig {
    /// Hibernate after this many seconds of idle time.
    pub idle_timeout_secs: u64,
    /// Directory to persist state files.
    pub state_dir: String,
    /// Optional HTTP endpoint that triggers wake.
    pub wake_endpoint: Option<String>,
    /// Maximum time to stay hibernated (seconds).
    pub max_hibernate_secs: u64,
}

impl Default for ServerlessConfig {
    fn default() -> Self {
        Self {
            idle_timeout_secs: 300, // 5 minutes
            state_dir: ".rustclaw/hibernated".to_string(),
            wake_endpoint: None,
            max_hibernate_secs: 86400 * 7, // 7 days
        }
    }
}

/// Serialized agent state for hibernation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    /// Session key identifying the agent.
    pub session_key: String,
    /// Model used by the agent.
    pub model: String,
    /// Serialized memory state (e.g., bincode-encoded session messages).
    pub memory_snapshot: Vec<u8>,
    /// Serialized pending tasks.
    pub pending_tasks: Vec<String>,
    /// Hash of config to detect changes.
    pub config_hash: String,
    /// Unix timestamp when hibernated (milliseconds).
    pub hibernated_at: i64,
    /// Number of times this agent has been woken.
    pub wake_count: u64,
}

impl AgentState {
    /// Create a new agent state for hibernation.
    pub fn new(
        session_key: &str,
        model: &str,
        memory_snapshot: Vec<u8>,
        pending_tasks: Vec<String>,
        config_hash: &str,
    ) -> Self {
        Self {
            session_key: session_key.to_string(),
            model: model.to_string(),
            memory_snapshot,
            pending_tasks,
            config_hash: config_hash.to_string(),
            hibernated_at: chrono::Utc::now().timestamp_millis(),
            wake_count: 0,
        }
    }

    /// Check if configuration has changed since hibernation.
    pub fn config_changed(&self, current_hash: &str) -> bool {
        self.config_hash != current_hash
    }

    /// Get age of hibernated state in seconds.
    pub fn age_secs(&self) -> u64 {
        let now = chrono::Utc::now().timestamp_millis();
        ((now - self.hibernated_at) / 1000) as u64
    }
}

/// Events that can wake a hibernated agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeEvent {
    /// Message received for a session.
    MessageReceived {
        session_key: String,
        message: String,
    },
    /// Cron job triggered.
    CronTrigger {
        job_id: String,
    },
    /// Webhook received.
    WebhookReceived {
        payload: serde_json::Value,
    },
    /// Explicit wake request.
    WakeRequested,
}

impl ServerlessRuntime {
    /// Create a new serverless runtime.
    pub fn new(config: ServerlessConfig) -> Self {
        let state_dir = PathBuf::from(&config.state_dir);

        tracing::info!(
            "Serverless runtime initialized: state_dir={}, idle_timeout={}s",
            config.state_dir,
            config.idle_timeout_secs
        );

        Self { state_dir, config }
    }

    /// Ensure state directory exists.
    async fn ensure_state_dir(&self) -> anyhow::Result<()> {
        if !self.state_dir.exists() {
            fs::create_dir_all(&self.state_dir).await?;
            tracing::debug!("Created state directory: {:?}", self.state_dir);
        }
        Ok(())
    }

    /// Get state file path for a session.
    fn state_file_path(&self, session_key: &str) -> PathBuf {
        // Sanitize session key for filename
        let safe_key = session_key
            .replace('/', "_")
            .replace(':', "_")
            .replace('\\', "_");
        self.state_dir.join(format!("{}.state.json", safe_key))
    }

    /// Hibernate an agent's state to disk.
    ///
    /// Returns the path to the state file.
    pub async fn hibernate(&self, state: AgentState) -> anyhow::Result<PathBuf> {
        self.ensure_state_dir().await?;

        let path = self.state_file_path(&state.session_key);
        let json = serde_json::to_string_pretty(&state)?;

        let mut file = fs::File::create(&path).await?;
        file.write_all(json.as_bytes()).await?;
        file.flush().await?;

        tracing::info!(
            "Hibernated agent '{}' to {:?} ({} bytes memory, {} pending tasks)",
            state.session_key,
            path,
            state.memory_snapshot.len(),
            state.pending_tasks.len()
        );

        Ok(path)
    }

    /// Resume an agent's state from disk.
    ///
    /// Returns `None` if no state file exists.
    pub async fn resume(&self, session_key: &str) -> anyhow::Result<Option<AgentState>> {
        let path = self.state_file_path(session_key);

        if !path.exists() {
            tracing::debug!("No hibernated state for '{}'", session_key);
            return Ok(None);
        }

        let json = fs::read_to_string(&path).await?;
        let mut state: AgentState = serde_json::from_str(&json)?;

        // Increment wake count
        state.wake_count += 1;

        // Delete state file after loading
        fs::remove_file(&path).await?;

        tracing::info!(
            "Resumed agent '{}' (wake #{}, hibernated {} seconds ago)",
            session_key,
            state.wake_count,
            state.age_secs()
        );

        Ok(Some(state))
    }

    /// Check if an agent should hibernate based on idle time.
    pub async fn should_hibernate(&self, last_activity: Instant) -> bool {
        let idle_secs = last_activity.elapsed().as_secs();
        idle_secs >= self.config.idle_timeout_secs
    }

    /// Clean up hibernated states older than max_age_secs.
    ///
    /// Returns the number of states cleaned up.
    pub async fn cleanup_old_states(&self, max_age_secs: u64) -> anyhow::Result<usize> {
        self.ensure_state_dir().await?;

        let mut entries = fs::read_dir(&self.state_dir).await?;
        let mut cleaned = 0;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            // Read state to check age
            match fs::read_to_string(&path).await {
                Ok(json) => {
                    if let Ok(state) = serde_json::from_str::<AgentState>(&json) {
                        if state.age_secs() > max_age_secs {
                            fs::remove_file(&path).await?;
                            tracing::info!(
                                "Cleaned up old state '{}' (age: {} seconds)",
                                state.session_key,
                                state.age_secs()
                            );
                            cleaned += 1;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to read state file {:?}: {}", path, e);
                }
            }
        }

        if cleaned > 0 {
            tracing::info!("Cleaned up {} old hibernated states", cleaned);
        }

        Ok(cleaned)
    }

    /// List all hibernated sessions with their timestamps.
    pub fn list_hibernated(&self) -> anyhow::Result<Vec<(String, i64)>> {
        let mut results = Vec::new();

        if !self.state_dir.exists() {
            return Ok(results);
        }

        for entry in std::fs::read_dir(&self.state_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            match std::fs::read_to_string(&path) {
                Ok(json) => {
                    if let Ok(state) = serde_json::from_str::<AgentState>(&json) {
                        results.push((state.session_key, state.hibernated_at));
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to read state file {:?}: {}", path, e);
                }
            }
        }

        // Sort by hibernation time (oldest first)
        results.sort_by_key(|(_, ts)| *ts);

        Ok(results)
    }

    /// Start HTTP wake server.
    ///
    /// Accepts POST /wake with RuntimeEvent payload.
    /// 
    /// Note: Requires axum dependency. Add to Cargo.toml:
    /// ```toml
    /// axum = "0.8"
    /// ```
    pub async fn start_wake_server(&self, port: u16) -> anyhow::Result<()> {
        use axum::{
            extract::State,
            http::StatusCode,
            routing::post,
            Json, Router,
        };
        use std::sync::Arc;

        #[derive(Clone)]
        struct WakeState {
            state_dir: PathBuf,
        }

        async fn handle_wake(
            State(state): State<Arc<WakeState>>,
            Json(event): Json<RuntimeEvent>,
        ) -> Result<Json<serde_json::Value>, StatusCode> {
            tracing::info!("Wake request received: {:?}", event);

            let session_key = match &event {
                RuntimeEvent::MessageReceived { session_key, .. } => Some(session_key.clone()),
                _ => None,
            };

            // Check if we have state to resume
            let has_state = if let Some(key) = &session_key {
                let safe_key = key
                    .replace('/', "_")
                    .replace(':', "_")
                    .replace('\\', "_");
                let path = state.state_dir.join(format!("{}.state.json", safe_key));
                path.exists()
            } else {
                false
            };

            Ok(Json(serde_json::json!({
                "status": "ok",
                "session_key": session_key,
                "has_hibernated_state": has_state,
            })))
        }

        async fn health() -> &'static str {
            "ok"
        }

        let wake_state = Arc::new(WakeState {
            state_dir: self.state_dir.clone(),
        });

        let app = Router::new()
            .route("/wake", post(handle_wake))
            .route("/health", axum::routing::get(health))
            .with_state(wake_state);

        let addr: std::net::SocketAddr = ([0, 0, 0, 0], port).into();
        tracing::info!("Starting wake server on {}", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }

    /// Get the configured idle timeout.
    pub fn idle_timeout_secs(&self) -> u64 {
        self.config.idle_timeout_secs
    }

    /// Get the configured max hibernate time.
    pub fn max_hibernate_secs(&self) -> u64 {
        self.config.max_hibernate_secs
    }

    /// Check if a session has hibernated state.
    pub fn has_hibernated_state(&self, session_key: &str) -> bool {
        self.state_file_path(session_key).exists()
    }
}

/// Helper to compute config hash for change detection.
pub fn compute_config_hash(config: &impl Serialize) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let json = serde_json::to_string(config).unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    json.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Manager for tracking activity and triggering hibernation.
pub struct ActivityTracker {
    last_activity: Instant,
    session_key: String,
}

impl ActivityTracker {
    /// Create a new activity tracker.
    pub fn new(session_key: &str) -> Self {
        Self {
            last_activity: Instant::now(),
            session_key: session_key.to_string(),
        }
    }

    /// Record activity (resets idle timer).
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Get seconds since last activity.
    pub fn idle_secs(&self) -> u64 {
        self.last_activity.elapsed().as_secs()
    }

    /// Get the session key.
    pub fn session_key(&self) -> &str {
        &self.session_key
    }

    /// Get the last activity instant.
    pub fn last_activity(&self) -> Instant {
        self.last_activity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_agent_state_new() {
        let state = AgentState::new(
            "test:session",
            "claude-3-opus",
            vec![1, 2, 3],
            vec!["task1".to_string()],
            "abc123",
        );

        assert_eq!(state.session_key, "test:session");
        assert_eq!(state.model, "claude-3-opus");
        assert_eq!(state.memory_snapshot, vec![1, 2, 3]);
        assert_eq!(state.pending_tasks, vec!["task1"]);
        assert_eq!(state.config_hash, "abc123");
        assert_eq!(state.wake_count, 0);
    }

    #[test]
    fn test_agent_state_config_changed() {
        let state = AgentState::new("test", "model", vec![], vec![], "hash1");

        assert!(!state.config_changed("hash1"));
        assert!(state.config_changed("hash2"));
    }

    #[test]
    fn test_runtime_event_serialization() {
        let event = RuntimeEvent::MessageReceived {
            session_key: "user:123".to_string(),
            message: "Hello".to_string(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("message_received"));
        assert!(json.contains("user:123"));

        let parsed: RuntimeEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            RuntimeEvent::MessageReceived { session_key, message } => {
                assert_eq!(session_key, "user:123");
                assert_eq!(message, "Hello");
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_compute_config_hash() {
        let config1 = serde_json::json!({"key": "value"});
        let config2 = serde_json::json!({"key": "value"});
        let config3 = serde_json::json!({"key": "different"});

        let hash1 = compute_config_hash(&config1);
        let hash2 = compute_config_hash(&config2);
        let hash3 = compute_config_hash(&config3);

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_activity_tracker() {
        let mut tracker = ActivityTracker::new("test:session");

        assert_eq!(tracker.session_key(), "test:session");
        assert!(tracker.idle_secs() < 1);

        tracker.touch();
        assert!(tracker.idle_secs() < 1);
    }

    #[tokio::test]
    async fn test_hibernate_and_resume() {
        let dir = tempdir().unwrap();
        let config = ServerlessConfig {
            state_dir: dir.path().to_string_lossy().to_string(),
            idle_timeout_secs: 60,
            wake_endpoint: None,
            max_hibernate_secs: 3600,
        };

        let runtime = ServerlessRuntime::new(config);

        // Create state
        let state = AgentState::new(
            "test:session:1",
            "claude-3-opus",
            vec![1, 2, 3, 4, 5],
            vec!["task1".to_string(), "task2".to_string()],
            "config_hash_123",
        );

        // Hibernate
        let path = runtime.hibernate(state.clone()).await.unwrap();
        assert!(path.exists());

        // Resume
        let resumed = runtime.resume("test:session:1").await.unwrap().unwrap();
        assert_eq!(resumed.session_key, "test:session:1");
        assert_eq!(resumed.model, "claude-3-opus");
        assert_eq!(resumed.memory_snapshot, vec![1, 2, 3, 4, 5]);
        assert_eq!(resumed.wake_count, 1); // Incremented on resume

        // State file should be deleted
        assert!(!path.exists());

        // Resume again should return None
        let none = runtime.resume("test:session:1").await.unwrap();
        assert!(none.is_none());
    }

    #[tokio::test]
    async fn test_list_hibernated() {
        let dir = tempdir().unwrap();
        let config = ServerlessConfig {
            state_dir: dir.path().to_string_lossy().to_string(),
            idle_timeout_secs: 60,
            wake_endpoint: None,
            max_hibernate_secs: 3600,
        };

        let runtime = ServerlessRuntime::new(config);

        // Initially empty
        let list = runtime.list_hibernated().unwrap();
        assert!(list.is_empty());

        // Hibernate some states
        runtime
            .hibernate(AgentState::new("session1", "model", vec![], vec![], "h"))
            .await
            .unwrap();
        runtime
            .hibernate(AgentState::new("session2", "model", vec![], vec![], "h"))
            .await
            .unwrap();

        let list = runtime.list_hibernated().unwrap();
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn test_should_hibernate() {
        let dir = tempdir().unwrap();
        let config = ServerlessConfig {
            state_dir: dir.path().to_string_lossy().to_string(),
            idle_timeout_secs: 1, // 1 second timeout for testing
            wake_endpoint: None,
            max_hibernate_secs: 3600,
        };

        let runtime = ServerlessRuntime::new(config);

        let recent = Instant::now();
        assert!(!runtime.should_hibernate(recent).await);

        // Wait a bit and check again
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        assert!(runtime.should_hibernate(recent).await);
    }

    #[tokio::test]
    async fn test_has_hibernated_state() {
        let dir = tempdir().unwrap();
        let config = ServerlessConfig {
            state_dir: dir.path().to_string_lossy().to_string(),
            idle_timeout_secs: 60,
            wake_endpoint: None,
            max_hibernate_secs: 3600,
        };

        let runtime = ServerlessRuntime::new(config);

        assert!(!runtime.has_hibernated_state("test:session"));

        runtime
            .hibernate(AgentState::new("test:session", "model", vec![], vec![], "h"))
            .await
            .unwrap();

        assert!(runtime.has_hibernated_state("test:session"));
    }

    #[test]
    fn test_state_file_path_sanitization() {
        let config = ServerlessConfig {
            state_dir: "/tmp/states".to_string(),
            ..Default::default()
        };
        let runtime = ServerlessRuntime::new(config);

        // Test various session key formats
        let path1 = runtime.state_file_path("user:123");
        assert!(path1.to_string_lossy().contains("user_123"));

        let path2 = runtime.state_file_path("agent/subagent/task");
        assert!(path2.to_string_lossy().contains("agent_subagent_task"));
    }
}
