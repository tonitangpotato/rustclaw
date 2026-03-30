//! Shared voice mode state — persisted to disk, accessible by tools and channels.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared voice mode manager. Thread-safe, persisted to disk.
#[derive(Clone)]
pub struct VoiceMode {
    state: Arc<Mutex<HashMap<i64, bool>>>,
    path: PathBuf,
}

impl VoiceMode {
    /// Load from disk or create empty.
    pub fn new(path: PathBuf) -> Self {
        let state = if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
                Err(_) => HashMap::new(),
            }
        } else {
            HashMap::new()
        };
        Self {
            state: Arc::new(Mutex::new(state)),
            path,
        }
    }

    /// Check if voice mode is active for a chat.
    pub async fn is_enabled(&self, chat_id: i64) -> bool {
        self.state.lock().await.get(&chat_id).copied().unwrap_or(false)
    }

    /// Set voice mode for a chat. Persists to disk.
    pub async fn set(&self, chat_id: i64, enabled: bool) {
        let mut map = self.state.lock().await;
        if enabled {
            map.insert(chat_id, true);
        } else {
            map.remove(&chat_id);
        }
        tracing::info!("Voice mode for chat {}: {}", chat_id, if enabled { "ON" } else { "OFF" });
        if let Ok(data) = serde_json::to_string(&*map) {
            let _ = std::fs::write(&self.path, data);
        }
    }

    /// Parse chat_id from session key (e.g., "telegram:7539582820" → 7539582820).
    pub fn chat_id_from_session(session_key: &str) -> Option<i64> {
        session_key.split(':').nth(1)?.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_voice_mode_toggle() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let vm = VoiceMode::new(tmp.path().to_path_buf());

        assert!(!vm.is_enabled(123).await);
        vm.set(123, true).await;
        assert!(vm.is_enabled(123).await);
        vm.set(123, false).await;
        assert!(!vm.is_enabled(123).await);
    }

    #[tokio::test]
    async fn test_persistence() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();

        {
            let vm = VoiceMode::new(path.clone());
            vm.set(456, true).await;
        }

        // Reload from disk
        let vm2 = VoiceMode::new(path);
        assert!(vm2.is_enabled(456).await);
    }

    #[test]
    fn test_chat_id_from_session() {
        assert_eq!(VoiceMode::chat_id_from_session("telegram:7539582820"), Some(7539582820));
        assert_eq!(VoiceMode::chat_id_from_session("discord:123"), Some(123));
        assert_eq!(VoiceMode::chat_id_from_session("invalid"), None);
    }
}
