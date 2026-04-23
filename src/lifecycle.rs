//! Process lifecycle notifications — tell Telegram when we're dying / reborn.
//!
//! Purpose: prevent "silent die" where RustClaw restarts or crashes without
//! the user being notified on Telegram. Writes a shutdown state file before
//! exit so the next startup can detect dirty vs clean shutdown.
//!
//! State file: `~/.rustclaw/last_shutdown-{instance}.json`
//! - `{"reason": "restart:recompiled binary", "clean": true, "ts": "..."}` — clean
//! - file missing or `clean: false` — dirty (SIGKILL / OOM / panic)
//!
//! The `{instance}` suffix is derived from the config file path so multiple
//! rustclaw instances (e.g. main agent + marketing agent) don't share a marker
//! and trigger false "dirty shutdown" alerts for each other. Call
//! [`set_instance_id`] once at startup BEFORE reading/writing markers.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Per-instance identifier used in the shutdown marker filename.
/// Set once at startup from the config file basename. If unset (tests, early
/// init), falls back to "default" so the old single-instance path still works.
static INSTANCE_ID: OnceLock<String> = OnceLock::new();

/// Install the instance id used for shutdown marker filenames. Called once at
/// startup with the `--config` path. Safe no-op if already set.
///
/// Derivation: take the config file stem (e.g. "rustclaw-marketing.yaml"
/// → "rustclaw-marketing"), lowercase, keep [a-z0-9._-] only. An empty or
/// invalid path becomes "default".
pub fn set_instance_id(config_path: &str) {
    let id = derive_instance_id(config_path);
    // OK if already set (e.g. two init paths race); first write wins.
    let _ = INSTANCE_ID.set(id);
}

fn derive_instance_id(config_path: &str) -> String {
    let stem = Path::new(config_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("default")
        .to_lowercase();
    let sanitized: String = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' })
        .collect();
    if sanitized.is_empty() {
        "default".to_string()
    } else {
        sanitized
    }
}

/// Global lifecycle notifier — set once at startup from cfg.telegram, read
/// anywhere (e.g. `restart_self` tool) to broadcast shutdown messages.
static NOTIFIER: OnceLock<LifecycleNotifier> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct LifecycleNotifier {
    pub bot_token: String,
    /// Explicit `notify_chat_ids` from config (highest priority).
    pub configured: Vec<i64>,
    /// Legacy auth whitelist, kept as last-resort fallback so existing
    /// setups that relied on the old (buggy) behavior don't regress.
    pub allowed_users: Vec<i64>,
}

/// Install the global notifier. Safe to call once at startup. Returns
/// `false` if already initialized or telegram is not configured.
///
/// Unlike the old version, this does NOT require any list to be non-empty —
/// autodiscovery (via `notify_targets::record_chat` on inbound messages) can
/// populate recipients at runtime. We still need a bot token to actually
/// send anything.
pub fn install_notifier(
    bot_token: String,
    configured: Vec<i64>,
    allowed_users: Vec<i64>,
) -> bool {
    if bot_token.is_empty() {
        tracing::warn!("Lifecycle notifier NOT installed: bot_token empty");
        return false;
    }
    let installed = NOTIFIER
        .set(LifecycleNotifier {
            bot_token,
            configured,
            allowed_users,
        })
        .is_ok();
    if installed {
        // Resolve once at install time just so we can log the initial state.
        // Actual broadcasts re-resolve to pick up autodiscovered chats.
        let autodisc = crate::notify_targets::load_autodiscovered();
        let n = NOTIFIER.get().unwrap();
        let (ids, src) = crate::notify_targets::resolve_recipients(
            &n.configured,
            &autodisc,
            &n.allowed_users,
        );
        if ids.is_empty() {
            tracing::warn!(
                "Lifecycle notifier installed but has 0 recipients \
                 (no notify_chat_ids, no autodiscovered, no allowed_users). \
                 Send a message to the bot once to bootstrap autodiscovery."
            );
        } else {
            tracing::info!(
                "Lifecycle notifier installed: {} recipient(s) via {}",
                ids.len(),
                src.as_str()
            );
        }
    }
    installed
}

/// Get the installed notifier, if any.
pub fn notifier() -> Option<&'static LifecycleNotifier> {
    NOTIFIER.get()
}

/// Record of the last shutdown for startup detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShutdownRecord {
    pub reason: String,
    pub clean: bool,
    pub ts: String,
}

fn state_file_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let instance = INSTANCE_ID.get().map(|s| s.as_str()).unwrap_or("default");
    PathBuf::from(home)
        .join(".rustclaw")
        .join(format!("last_shutdown-{}.json", instance))
}

/// Write a shutdown marker BEFORE calling exit. Call with `clean: true` for
/// intentional shutdowns (SIGTERM, restart_self) and never with `clean: false`
/// (dirty shutdowns leave no marker so startup detects absence = crash).
pub fn mark_shutdown(reason: &str, clean: bool) {
    let record = ShutdownRecord {
        reason: reason.to_string(),
        clean,
        ts: chrono::Local::now().to_rfc3339(),
    };
    let path = state_file_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(&record) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                tracing::warn!("Failed to write shutdown marker {}: {}", path.display(), e);
            }
        }
        Err(e) => tracing::warn!("Failed to serialize shutdown marker: {}", e),
    }
}

/// Read and consume the previous shutdown marker. Returns None if file missing
/// (= dirty shutdown) or unreadable. The file is deleted after reading so the
/// NEXT startup detects its own state correctly.
pub fn take_previous_shutdown() -> Option<ShutdownRecord> {
    let path = state_file_path();
    let content = std::fs::read_to_string(&path).ok()?;
    let record: ShutdownRecord = serde_json::from_str(&content).ok()?;
    let _ = std::fs::remove_file(&path);
    Some(record)
}

/// Send a plain text message to a Telegram chat via HTTP API. Synchronous
/// (blocking reqwest) so it works from SIGTERM handlers and exit paths where
/// async tokio runtime may be shutting down.
///
/// Returns `Ok(())` on HTTP 200, error otherwise. Failures are logged but
/// not propagated — notification is best-effort.
pub fn send_telegram_sync(bot_token: &str, chat_id: i64, text: &str) -> anyhow::Result<()> {
    let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    let response = client
        .post(&url)
        .json(&serde_json::json!({
            "chat_id": chat_id,
            "text": text,
        }))
        .send()?;
    if !response.status().is_success() {
        anyhow::bail!("Telegram API error: {}", response.status());
    }
    Ok(())
}

/// Broadcast a lifecycle message to all allowed Telegram users. Best-effort —
/// logs errors but doesn't fail.
pub fn broadcast_lifecycle_message(bot_token: &str, allowed_users: &[i64], text: &str) {
    if bot_token.is_empty() || allowed_users.is_empty() {
        return;
    }
    for &chat_id in allowed_users {
        match send_telegram_sync(bot_token, chat_id, text) {
            Ok(_) => tracing::info!("Lifecycle message sent to chat {}: {}", chat_id, text),
            Err(e) => tracing::warn!("Failed to send lifecycle message to {}: {}", chat_id, e),
        }
    }
}

/// Broadcast via the globally installed notifier. No-op if not installed.
/// Re-resolves recipients at every call so autodiscovered chats added after
/// startup are picked up without a restart.
pub fn broadcast(text: &str) {
    let Some(n) = notifier() else {
        tracing::debug!("Lifecycle broadcast skipped: notifier not installed");
        return;
    };
    let autodisc = crate::notify_targets::load_autodiscovered();
    let (ids, src) = crate::notify_targets::resolve_recipients(
        &n.configured,
        &autodisc,
        &n.allowed_users,
    );
    if ids.is_empty() {
        tracing::warn!(
            "Lifecycle broadcast SKIPPED ('{}'): 0 recipients. \
             Source: {}. Ensure someone has messaged the bot (autodiscovery) \
             or set telegram.notify_chat_ids in rustclaw.yaml.",
            text,
            src.as_str()
        );
        return;
    }
    tracing::info!(
        "Lifecycle broadcast → {} recipient(s) via {}: {}",
        ids.len(),
        src.as_str(),
        text
    );
    broadcast_lifecycle_message(&n.bot_token, &ids, text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_record_roundtrip() {
        let rec = ShutdownRecord {
            reason: "test".to_string(),
            clean: true,
            ts: "2026-04-21T23:00:00-04:00".to_string(),
        };
        let json = serde_json::to_string(&rec).unwrap();
        let parsed: ShutdownRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.reason, "test");
        assert!(parsed.clean);
    }

    #[test]
    fn derive_instance_id_basic_cases() {
        // Typical config paths produce clean basenames.
        assert_eq!(derive_instance_id("rustclaw.yaml"), "rustclaw");
        assert_eq!(derive_instance_id("rustclaw-2.yaml"), "rustclaw-2");
        assert_eq!(
            derive_instance_id("rustclaw-marketing.yaml"),
            "rustclaw-marketing"
        );
        // Absolute paths work.
        assert_eq!(
            derive_instance_id("/Users/potato/rustclaw/rustclaw.yaml"),
            "rustclaw"
        );
        // Different config suffixes give different ids → no collision.
        let a = derive_instance_id("/x/rustclaw.yaml");
        let b = derive_instance_id("/x/rustclaw-2.yaml");
        let c = derive_instance_id("/x/rustclaw-marketing.yaml");
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(b, c);
    }

    #[test]
    fn derive_instance_id_sanitizes_weird_input() {
        // Unicode / spaces / slashes in stem get replaced with '-'.
        let id = derive_instance_id("/tmp/weird name!@#.yaml");
        assert!(!id.is_empty());
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'));
        // Empty path falls back to "default".
        assert_eq!(derive_instance_id(""), "default");
        // Uppercase is normalized so two instances that differ only in case collide,
        // but that's a file-system-friendly trade-off (macOS is case-insensitive anyway).
        assert_eq!(derive_instance_id("RustClaw.YAML"), "rustclaw");
    }
}
