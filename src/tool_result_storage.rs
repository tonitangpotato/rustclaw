//! Tool result persistence — large results are stored on disk
//! and replaced with a preview in context.
//!
//! Inspired by Claude Code's toolResultStorage.ts:
//! - Results > threshold are persisted to ~/.rustclaw/tool-results/{session}/
//! - Context gets preview (first N chars) + file path
//! - Agent can read_file to access full content when needed

use std::path::{Path, PathBuf};

/// Default threshold: persist results larger than this (chars).
const DEFAULT_PERSIST_THRESHOLD: usize = 30_000;

/// Preview size: keep this many chars in context.
const PREVIEW_CHARS: usize = 2000;

/// Base directory for persisted tool results.
fn results_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".rustclaw")
        .join("tool-results")
}

/// Check if a tool result should be persisted to disk.
pub fn should_persist(content: &str) -> bool {
    content.len() > DEFAULT_PERSIST_THRESHOLD
}

/// Persist a large tool result to disk and return the replacement content.
///
/// Returns (preview_content, file_path) on success.
/// On failure, returns None and the original content should be kept.
pub fn persist_and_preview(
    session_key: &str,
    tool_call_id: &str,
    tool_name: &str,
    content: &str,
) -> Option<(String, PathBuf)> {
    // Sanitize session key for filesystem
    let safe_session = session_key.replace([':', '/', '\\'], "_");
    let dir = results_dir().join(&safe_session);

    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!("Failed to create tool-results dir: {}", e);
        return None;
    }

    let file_name = format!("{}.txt", tool_call_id);
    let file_path = dir.join(&file_name);

    if let Err(e) = std::fs::write(&file_path, content) {
        tracing::warn!("Failed to persist tool result: {}", e);
        return None;
    }

    let preview_end = content.len().min(PREVIEW_CHARS);
    // Safety: ensure we don't split a multi-byte char
    let preview_end = content.floor_char_boundary(preview_end);
    let preview = &content[..preview_end];

    let replacement = format!(
        "{}...\n\n[Full output persisted: {} ({} chars) — use read_file to access]",
        preview,
        file_path.display(),
        content.len()
    );

    tracing::info!(
        "Persisted tool result for {} ({} chars) to {}",
        tool_name,
        content.len(),
        file_path.display()
    );

    Some((replacement, file_path))
}

/// Clean up persisted results for a session.
pub fn cleanup_session(session_key: &str) {
    let safe_session = session_key.replace([':', '/', '\\'], "_");
    let dir = results_dir().join(&safe_session);

    if dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            tracing::warn!("Failed to cleanup tool-results for {}: {}", session_key, e);
        } else {
            tracing::debug!("Cleaned up tool-results for {}", session_key);
        }
    }
}

/// Clean up all stale tool result directories (for sessions that no longer exist).
pub fn cleanup_stale(active_sessions: &[String]) {
    let base = results_dir();
    if !base.exists() {
        return;
    }

    let active_set: std::collections::HashSet<String> = active_sessions
        .iter()
        .map(|s| s.replace([':', '/', '\\'], "_"))
        .collect();

    if let Ok(entries) = std::fs::read_dir(&base) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if !active_set.contains(name) {
                    let _ = std::fs::remove_dir_all(entry.path());
                    tracing::debug!("Cleaned up stale tool-results: {}", name);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_persist() {
        let short = "a".repeat(1000);
        assert!(!should_persist(&short));

        let long = "a".repeat(31_000);
        assert!(should_persist(&long));
    }

    #[test]
    fn test_persist_and_preview() {
        let content = "x".repeat(35_000);
        let result = persist_and_preview("test_session", "call_123", "web_fetch", &content);
        assert!(result.is_some());

        let (preview, path) = result.unwrap();
        assert!(preview.contains("[Full output persisted:"));
        assert!(preview.len() < content.len());
        assert!(path.exists());

        // Cleanup
        cleanup_session("test_session");
        assert!(!path.exists());
    }
}
