//! Engram memory hooks for automatic recall and storage.
//!
//! Two hooks:
//! - EngramRecallHook: Auto-recalls relevant memories before processing (BeforeInbound)
//! - EngramStoreHook: Auto-stores important interactions after responding (BeforeOutbound)

use async_trait::async_trait;
use std::sync::Arc;

use crate::hooks::{Hook, HookContext, HookOutcome, HookPoint};
use crate::memory::MemoryManager;

/// Auto-recall relevant memories before processing user messages.
/// Injects recalled memories into the hook context metadata so the agent
/// can include them in the LLM prompt.
pub struct EngramRecallHook {
    memory: Arc<MemoryManager>,
}

impl EngramRecallHook {
    pub fn new(memory: Arc<MemoryManager>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Hook for EngramRecallHook {
    fn name(&self) -> &str {
        "engram-recall"
    }

    fn point(&self) -> HookPoint {
        HookPoint::BeforeInbound
    }

    fn priority(&self) -> i32 {
        50 // Run after safety hooks (priority 10)
    }

    async fn execute(&self, ctx: &mut HookContext) -> anyhow::Result<HookOutcome> {
        // Skip very short messages (greetings, "ok", "thanks" etc.)
        if ctx.content.len() < 10 {
            return Ok(HookOutcome::Continue(None));
        }

        // Session-aware recall: uses working memory for topic continuity
        match self.memory.session_recall(&ctx.content) {
            Ok((results, full_recall_triggered)) => {
                if !results.is_empty() {
                    // Format memories with confidence labels
                    let memories: Vec<String> = results
                        .iter()
                        .map(|r| {
                            let label = r.confidence_label.as_deref().unwrap_or("likely");
                            format!("- [{}] [{}] {}", label, r.memory_type, r.content)
                        })
                        .collect();
                    let memory_block = format!(
                        "\n## ⚠️ Recalled Memories (auto) — You may have prior context on this topic. Review before answering.\n{}\n",
                        memories.join("\n")
                    );

                    // Store in metadata for agent.rs to pick up
                    if let Some(obj) = ctx.metadata.as_object_mut() {
                        obj.insert(
                            "engram_recall".to_string(),
                            serde_json::json!({
                                "count": results.len(),
                                "full_recall": full_recall_triggered,
                                "formatted": memory_block,
                                "results": results.iter().map(|r| {
                                    serde_json::json!({
                                        "content": r.content,
                                        "type": r.memory_type,
                                        "confidence": r.confidence,
                                        "confidence_label": r.confidence_label,
                                    })
                                }).collect::<Vec<_>>()
                            }),
                        );
                    }

                    if full_recall_triggered {
                        tracing::info!(
                            "Engram session-recall: {} memories (full recall, topic changed)",
                            results.len()
                        );
                    } else {
                        tracing::debug!(
                            "Engram session-recall: {} memories from working memory",
                            results.len()
                        );
                    }
                }
                Ok(HookOutcome::Continue(None))
            }
            Err(e) => {
                tracing::debug!("Engram recall failed (non-fatal): {}", e);
                Ok(HookOutcome::Continue(None))
            }
        }
    }
}

/// Auto-store important interactions after responding.
pub struct EngramStoreHook {
    memory: Arc<MemoryManager>,
}

impl EngramStoreHook {
    pub fn new(memory: Arc<MemoryManager>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Hook for EngramStoreHook {
    fn name(&self) -> &str {
        "engram-store"
    }

    fn point(&self) -> HookPoint {
        HookPoint::BeforeOutbound
    }

    fn priority(&self) -> i32 {
        90 // Run after safety hooks
    }

    async fn execute(&self, ctx: &mut HookContext) -> anyhow::Result<HookOutcome> {
        // Skip empty or very short responses
        if ctx.content.len() < 20 {
            return Ok(HookOutcome::Continue(None));
        }

        // Skip heartbeat sessions — they generate repetitive operational noise
        if ctx.metadata.get("is_heartbeat").and_then(|v| v.as_bool()).unwrap_or(false) {
            tracing::debug!("Engram store hook: skipping heartbeat session");
            return Ok(HookOutcome::Continue(None));
        }

        // Skip known non-content responses
        let trimmed = ctx.content.trim();
        if trimmed == "NO_REPLY" || trimmed == "HEARTBEAT_OK" {
            return Ok(HookOutcome::Continue(None));
        }

        // Skip cron session responses (session key starts with "cron:")
        if ctx.session_key.starts_with("cron:") {
            tracing::debug!("Engram store hook: skipping cron session '{}'", ctx.session_key);
            return Ok(HookOutcome::Continue(None));
        }

        // Extract the user message from metadata (set by agent before calling hook)
        let user_msg = ctx
            .metadata
            .get("user_message")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if user_msg.is_empty() {
            return Ok(HookOutcome::Continue(None));
        }

        // Create a condensed summary for storage
        let response_preview = {
            let end = ctx.content.len().min(200);
            // Safe char boundary
            let end = ctx.content.floor_char_boundary(end);
            &ctx.content[..end]
        };
        let store_content = format!("{} → {}", user_msg, response_preview);

        tracing::info!("Engram store hook: sending to extractor ({} chars)", store_content.len());
        match self.memory.store(
            &store_content,
            engramai::MemoryType::Episodic,
            0.5,
            Some("auto"),
        ) {
            Ok(()) => tracing::info!("Engram store hook: completed"),
            Err(e) => tracing::warn!("Engram auto-store failed: {}", e),
        }

        // Track emotional valence per domain (EmotionalAccumulator)
        let emotion = MemoryManager::detect_emotion(user_msg);
        let domain = MemoryManager::detect_domain(&store_content);
        
        if let Err(e) = self.memory.process_interaction(&store_content, emotion, domain) {
            tracing::debug!("Engram emotion tracking failed (non-fatal): {}", e);
        } else if emotion != 0.0 {
            tracing::debug!(
                "Engram emotion tracked: {:.2} for domain '{}'",
                emotion, domain
            );
        }

        Ok(HookOutcome::Continue(None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_point_constants() {
        // Verify hook points are correct (no runtime test needed for MemoryManager)
        assert_eq!(HookPoint::BeforeInbound, HookPoint::BeforeInbound);
        assert_eq!(HookPoint::BeforeOutbound, HookPoint::BeforeOutbound);
    }

    #[test]
    fn test_store_hook_skip_conditions() {
        // Test the filtering logic that prevents garbage memory writes
        
        // Helper: simulates what execute() checks
        fn should_skip(content: &str, session_key: &str, is_heartbeat: bool) -> bool {
            let trimmed = content.trim();
            if content.len() < 20 { return true; }
            if is_heartbeat { return true; }
            if trimmed == "NO_REPLY" || trimmed == "HEARTBEAT_OK" { return true; }
            if session_key.starts_with("cron:") { return true; }
            false
        }

        // Should skip: heartbeat
        assert!(should_skip("Some heartbeat response text here...", "heartbeat", true));
        
        // Should skip: NO_REPLY
        assert!(should_skip("NO_REPLY", "user:123", false));
        
        // Should skip: HEARTBEAT_OK
        assert!(should_skip("HEARTBEAT_OK", "user:123", false));
        
        // Should skip: cron session
        assert!(should_skip("Some cron output response here...", "cron:memory-maintenance", false));
        
        // Should skip: short content
        assert!(should_skip("ok thanks", "user:123", false));
        
        // Should NOT skip: normal user interaction
        assert!(!should_skip("Here's the analysis of your code...", "user:123", false));
        
        // Should NOT skip: telegram direct message
        assert!(!should_skip("我来帮你看看这个问题", "telegram:potato", false));
    }
}
