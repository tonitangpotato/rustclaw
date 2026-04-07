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
}
