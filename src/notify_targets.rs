//! Operational notification recipients — decoupled from auth whitelist.
//!
//! Problem this solves: `TelegramConfig.allowed_users` is an **auth filter**
//! ("who is allowed to talk to me", empty = allow all). Historically, two
//! subsystems (lifecycle notifier, heartbeat alerter) misused it as a
//! **notification destination list**. Result: when `allowed_users` is empty
//! (the "allow all" case), lifecycle broadcasts became silent no-ops, and
//! restart notifications never reached Telegram.
//!
//! Resolution order for operational recipients:
//!   1. Explicit config `TelegramConfig.notify_chat_ids` (highest priority)
//!   2. Autodiscovered chats (persisted on every inbound message)
//!   3. Legacy fallback to `allowed_users` (so existing setups don't regress)
//!   4. Empty → notifier disabled (logged at WARN)
//!
//! Autodiscovery file: `~/.rustclaw/notify_targets.json`
//! - Updated on every inbound Telegram message
//! - Dedup'd, bounded to MAX_RECENT chats (most-recently-seen order)

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Maximum number of autodiscovered chats we retain. Small cap — operational
/// notifications should reach recent, active conversations, not every chat
/// that ever DM'd the bot.
const MAX_RECENT: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct NotifyTargetsFile {
    /// Chat IDs in most-recently-seen order (index 0 = most recent).
    chats: Vec<i64>,
}

fn state_file_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join(".rustclaw")
        .join("notify_targets.json")
}

/// Load the autodiscovered chat list. Returns empty Vec on any error (file
/// missing, malformed, etc.) — autodiscovery is best-effort.
pub fn load_autodiscovered() -> Vec<i64> {
    let path = state_file_path();
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let file: NotifyTargetsFile = serde_json::from_str(&content).unwrap_or_default();
    file.chats
}

/// Record a chat_id as "seen recently". Moves it to the front (most-recent),
/// dedups, caps at MAX_RECENT. Best-effort — errors logged at debug, never
/// propagated. Call this on every inbound Telegram message.
pub fn record_chat(chat_id: i64) {
    if chat_id == 0 {
        return;
    }
    let path = state_file_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Read-modify-write. Small file, infrequent writes — no locking needed
    // for a single-process agent. If it becomes multi-process we can add
    // flock or move to SQLite.
    let mut file: NotifyTargetsFile = std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default();

    // Move chat_id to front; dedup.
    file.chats.retain(|&id| id != chat_id);
    file.chats.insert(0, chat_id);
    if file.chats.len() > MAX_RECENT {
        file.chats.truncate(MAX_RECENT);
    }

    match serde_json::to_string_pretty(&file) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                tracing::debug!("Failed to persist notify target {}: {}", path.display(), e);
            }
        }
        Err(e) => tracing::debug!("Failed to serialize notify targets: {}", e),
    }
}

/// Source of the resolved recipient list — logged at startup for observability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipientSource {
    /// Explicit `notify_chat_ids` from config.
    Configured,
    /// Autodiscovered from inbound messages (`~/.rustclaw/notify_targets.json`).
    Autodiscovered,
    /// Legacy: fell back to `allowed_users` (auth whitelist).
    LegacyAllowedUsers,
    /// Nothing available — notifier disabled.
    None,
}

impl RecipientSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            RecipientSource::Configured => "configured",
            RecipientSource::Autodiscovered => "autodiscovered",
            RecipientSource::LegacyAllowedUsers => "legacy(allowed_users)",
            RecipientSource::None => "none",
        }
    }
}

/// Resolve operational notification recipients using the priority order
/// documented at the top of this module. Pure function — inputs in, list out.
/// Easy to unit-test. Re-called at broadcast time so autodiscovery picks up
/// new chats without restart.
pub fn resolve_recipients(
    notify_chat_ids: &[i64],
    autodiscovered: &[i64],
    allowed_users: &[i64],
) -> (Vec<i64>, RecipientSource) {
    if !notify_chat_ids.is_empty() {
        return (notify_chat_ids.to_vec(), RecipientSource::Configured);
    }
    if !autodiscovered.is_empty() {
        return (autodiscovered.to_vec(), RecipientSource::Autodiscovered);
    }
    if !allowed_users.is_empty() {
        return (allowed_users.to_vec(), RecipientSource::LegacyAllowedUsers);
    }
    (Vec::new(), RecipientSource::None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_prefers_configured() {
        let (ids, src) = resolve_recipients(&[1, 2], &[3], &[4]);
        assert_eq!(ids, vec![1, 2]);
        assert_eq!(src, RecipientSource::Configured);
    }

    #[test]
    fn resolve_falls_back_to_autodiscovered() {
        let (ids, src) = resolve_recipients(&[], &[3, 4], &[5]);
        assert_eq!(ids, vec![3, 4]);
        assert_eq!(src, RecipientSource::Autodiscovered);
    }

    #[test]
    fn resolve_falls_back_to_legacy_allowed_users() {
        let (ids, src) = resolve_recipients(&[], &[], &[5, 6]);
        assert_eq!(ids, vec![5, 6]);
        assert_eq!(src, RecipientSource::LegacyAllowedUsers);
    }

    #[test]
    fn resolve_empty_when_nothing_available() {
        let (ids, src) = resolve_recipients(&[], &[], &[]);
        assert!(ids.is_empty());
        assert_eq!(src, RecipientSource::None);
    }

    #[test]
    fn source_as_str_is_stable() {
        assert_eq!(RecipientSource::Configured.as_str(), "configured");
        assert_eq!(RecipientSource::None.as_str(), "none");
    }

    #[test]
    fn record_chat_dedups_and_caps() {
        // Use a temp HOME so this test doesn't touch real state.
        let tmp = std::env::temp_dir().join(format!("rustclaw-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let old_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", &tmp);

        // Clear any leftover state.
        let _ = std::fs::remove_file(state_file_path());

        for id in 1..=12i64 {
            record_chat(id);
        }
        // Re-insert 3 to make sure it dedups and moves to front.
        record_chat(3);

        let chats = load_autodiscovered();
        assert_eq!(chats.len(), MAX_RECENT, "should be capped at MAX_RECENT");
        assert_eq!(chats[0], 3, "most recently recorded should be first");
        // 3 appears exactly once.
        assert_eq!(chats.iter().filter(|&&id| id == 3).count(), 1);
        // chat_id 0 is ignored.
        record_chat(0);
        let chats2 = load_autodiscovered();
        assert!(!chats2.contains(&0));

        // Restore HOME.
        if let Some(h) = old_home {
            std::env::set_var("HOME", h);
        } else {
            std::env::remove_var("HOME");
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
