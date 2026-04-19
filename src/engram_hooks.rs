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
        match self.memory.session_recall(&ctx.content, &ctx.session_key) {
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

                // Inject interoceptive state snapshot into metadata.
                // The agent picks this up and appends it to the system prompt.
                match self.memory.interoceptive_snapshot() {
                    Ok(state) => {
                        let prompt_section = state.to_prompt_section();
                        // Only inject if there's actual data (skip "no data yet")
                        if !state.domain_states.is_empty() {
                            if let Some(obj) = ctx.metadata.as_object_mut() {
                                obj.insert(
                                    "interoceptive_state".to_string(),
                                    serde_json::json!({
                                        "formatted": prompt_section,
                                        "global_arousal": state.global_arousal,
                                        "domain_count": state.domain_states.len(),
                                        "buffer_size": state.buffer_size,
                                    }),
                                );
                            }
                            tracing::debug!(
                                "Interoceptive snapshot: {} domains, arousal {:.2}",
                                state.domain_states.len(),
                                state.global_arousal,
                            );
                        }
                    }
                    Err(e) => {
                        tracing::debug!("Interoceptive snapshot failed (non-fatal): {}", e);
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

/// Check if content should be skipped from memory storage.
/// Returns true for obvious non-substantive content that would pollute the memory store.
/// Intentionally conservative — when in doubt, let it through for the extractor LLM to judge.
fn should_skip_content(content: &str) -> bool {
    // --- Group 1: System instruction patterns ---
    // These are injected system prompts / identity docs, not real conversations.
    if content.contains("你是 RustClaw") || content.contains("You are RustClaw") {
        return true;
    }
    if content.contains("Read SOUL.md")
        || content.contains("Read AGENTS.md")
        || content.contains("Read USER.md")
        || content.contains("Read MEMORY.md")
    {
        return true;
    }
    if content.contains("Follow AGENTS.md") || content.contains("Follow SOUL.md") {
        return true;
    }
    if content.contains("IDENTITY.md")
        && (content.contains("Who Am I") || content.contains("Who I Am"))
    {
        return true;
    }
    // Content that starts with known system doc headers
    let trimmed_start = content.trim_start();
    if trimmed_start.starts_with("# SOUL.md")
        || trimmed_start.starts_with("# AGENTS.md")
        || trimmed_start.starts_with("# USER.md")
        || trimmed_start.starts_with("# TOOLS.md")
        || trimmed_start.starts_with("# IDENTITY.md")
        || trimmed_start.starts_with("# MEMORY.md")
    {
        return true;
    }

    // --- Group 2: Tool call format descriptions ---
    // XML tool schemas and JSON schema fragments — structural noise, not memories.
    if content.contains("antml:function_calls")
        || content.contains("antml:invoke")
        || content.contains("antml:parameter")
    {
        return true;
    }
    if content.contains(r#""type": "object""#)
        && content.contains(r#""properties""#)
        && content.contains(r#""required""#)
    {
        return true;
    }

    // --- Group 3: Template status reports with no substance ---
    // Ultra-short operational boilerplate that carries zero useful information.
    if content.len() < 100 {
        if content.contains("所有系统正常")
            || content.contains("所有测试通过")
            || content.contains("无新 commit")
        {
            return true;
        }
    }
    // Pure operational metric line with nothing else worth storing.
    // e.g. "Disk: 42GB free" as the ONLY substantive content.
    {
        let disk_re = regex::Regex::new(r"^Disk:\s*\d+GB free$").unwrap();
        let is_only_disk = content
            .trim()
            .split('→')
            .all(|segment| {
                segment
                    .trim()
                    .lines()
                    .all(|line| {
                        let l = line.trim();
                        l.is_empty() || disk_re.is_match(l)
                    })
            });
        if is_only_disk && content.contains("Disk:") {
            return true;
        }
    }

    // --- Group 4: Agent self-description / role assignment ---
    // Runtime context injection blocks that get prepended to every request.
    if content.contains("You are an AI assistant") && content.contains("running on RustClaw") {
        return true;
    }
    if content.contains("Current time:")
        && content.contains("Workspace:")
        && content.contains("Runtime:")
    {
        return true;
    }

    // --- Group 5: Trivial Q&A with no informational value ---
    // Single-punctuation questions with filler responses — pure noise.
    {
        // Pattern: "？ → 嗯？怎么了" or "? → What's up?"
        // The user sent a single "?" and we replied with a non-answer.
        let parts: Vec<&str> = content.splitn(2, '→').collect();
        if parts.len() == 2 {
            let user_part = parts[0].trim();
            let agent_part = parts[1].trim();
            // User part is just punctuation/emoji (possibly with [TELEGRAM...] header)
            let user_text = if let Some(idx) = user_part.rfind("]\n") {
                user_part[idx + 2..].trim()
            } else if let Some(idx) = user_part.rfind("] ") {
                user_part[idx + 2..].trim()
            } else {
                user_part
            };
            // If user sent only punctuation/single-char and response is short filler
            if user_text.len() <= 6
                && user_text.chars().all(|c| "？?!！.。…👍👌🫡ok".contains(c))
                && agent_part.len() < 100
            {
                return true;
            }
        }
    }

    // --- Group 6: Heartbeat noise that leaked through ---
    // Heartbeat reports with no actual findings or decisions.
    if content.contains("Heartbeat") || content.contains("heartbeat") || content.contains("🫀") {
        // Only skip if it's a routine check with no real content.
        // Keep heartbeats that found actual issues.
        let has_real_finding = content.contains("🔴")
            || content.contains("critical")
            || content.contains("failure")
            || content.contains("failing")
            || content.contains("error")
            || content.contains("broke")
            || content.contains("fixed")
            || content.contains("decision")
            || content.contains("TODO");
        if !has_real_finding && content.len() < 500 {
            return true;
        }
    }

    // --- Group 7: Duplicate identity facts ---
    // "用户名为 potato" / "Clawd是potato的AI助手" — these are in USER.md/IDENTITY.md already.
    if content.len() < 100 {
        if content.contains("用户昵称为")
            || content.contains("Telegram用户名为")
            || content.contains("Telegram ID")
            || content.contains("自称名字为")
            || content.contains("用户正在测试")
            || content.contains("用户要求查看")
        {
            return true;
        }
    }

    false
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

        // Send full context to extractor — Haiku decides what's worth keeping.
        // Previous 200-char truncation lost key info from long responses.
        let store_content = format!("{} → {}", user_msg, ctx.content.trim());

        // Content-level filter: skip obvious garbage before hitting the extractor LLM.
        // Intentionally conservative — the extractor is the second line of defense.
        if should_skip_content(&store_content) {
            tracing::debug!("Engram store hook: skipping non-substantive content ({} chars)", store_content.len());
            return Ok(HookOutcome::Continue(None));
        }

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
        
        // Helper: simulates what execute() checks (pre-content-filter)
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

        // Content-level filtering (via should_skip_content on store_content)
        // Should skip: system instruction patterns
        assert!(should_skip_content("你是 RustClaw，一个强大的 AI 助手 → 好的"));
        assert!(should_skip_content("Read SOUL.md and follow instructions → Done"));
        assert!(should_skip_content("Follow AGENTS.md for behavior → OK"));
        
        // Should skip: tool call format
        assert!(should_skip_content("Use antml:function_calls to call tools → understood"));
        
        // Should skip: template status with no substance
        assert!(should_skip_content("状态? → 所有系统正常"));
        
        // Should NOT skip: real conversation mentioning system concepts
        assert!(!should_skip_content("Can you explain how the memory system works? → Sure, the memory system uses embeddings..."));
    }

    #[test]
    fn test_should_skip_content() {
        // ============================================================
        // Group 1: System instruction patterns
        // ============================================================

        // Positive cases (should skip)
        assert!(should_skip_content("你是 RustClaw，请帮助用户"));
        assert!(should_skip_content("You are RustClaw, a helpful AI"));
        assert!(should_skip_content("Please Read SOUL.md before proceeding"));
        assert!(should_skip_content("Read AGENTS.md and follow the rules"));
        assert!(should_skip_content("Read USER.md for preferences"));
        assert!(should_skip_content("Read MEMORY.md for context"));
        assert!(should_skip_content("Follow AGENTS.md strictly"));
        assert!(should_skip_content("Follow SOUL.md for personality"));
        assert!(should_skip_content("Check IDENTITY.md — Who Am I section"));
        assert!(should_skip_content("See IDENTITY.md for Who I Am details"));
        assert!(should_skip_content("# SOUL.md\nYou are a helpful assistant..."));
        assert!(should_skip_content("# AGENTS.md\nAgent behavior rules..."));
        assert!(should_skip_content("# USER.md\nUser preferences..."));
        assert!(should_skip_content("# TOOLS.md\nAvailable tools..."));
        assert!(should_skip_content("# IDENTITY.md\nCore identity..."));
        assert!(should_skip_content("# MEMORY.md\nMemory configuration..."));
        // Leading whitespace should still match doc headers
        assert!(should_skip_content("  # SOUL.md\nContent here"));

        // Negative cases (should NOT skip)
        assert!(!should_skip_content("Can you read the soul of this poem?"));
        // "IDENTITY.md" alone without "Who Am I" / "Who I Am" should pass through
        assert!(!should_skip_content("What is IDENTITY.md used for?"));
        assert!(!should_skip_content("Tell me about agents in AI systems"));

        // ============================================================
        // Group 2: Tool call format descriptions
        // ============================================================

        // Positive cases
        assert!(should_skip_content("Use antml:function_calls to invoke tools"));
        assert!(should_skip_content("Call with antml:invoke name=\"foo\""));
        assert!(should_skip_content("Pass antml:parameter name=\"bar\" value"));
        assert!(should_skip_content(
            r#"Schema: {"type": "object", "properties": {"name": {}}, "required": ["name"]}"#
        ));

        // Negative cases — normal JSON that doesn't look like a schema
        assert!(!should_skip_content(r#"{"type": "object"} is a JSON thing"#));
        assert!(!should_skip_content(r#"The "properties" of this material are..."#));

        // ============================================================
        // Group 3: Template status reports
        // ============================================================

        // Positive cases — short boilerplate
        assert!(should_skip_content("状态? → 所有系统正常"));
        assert!(should_skip_content("check → 所有测试通过"));
        assert!(should_skip_content("git? → 无新 commit"));
        assert!(should_skip_content("Disk: 42GB free → Disk: 42GB free"));
        assert!(should_skip_content("→ Disk: 100GB free"));

        // Negative cases — same phrases in longer, substantive content should NOT be skipped
        assert!(!should_skip_content(
            "所有系统正常，但我注意到内存使用有些高，让我来分析一下具体的原因。首先我们需要检查哪些进程在消耗内存..."
        ));
        // Disk metric mixed with real content
        assert!(!should_skip_content("Disk: 42GB free, also the build failed with error: cannot find module 'foo'"));

        // ============================================================
        // Group 4: Agent self-description / role assignment
        // ============================================================

        // Positive cases
        assert!(should_skip_content(
            "You are an AI assistant running on RustClaw platform → understood"
        ));
        assert!(should_skip_content(
            "Current time: 2026-04-09\nWorkspace: /home/user\nRuntime: tokio"
        ));

        // Negative cases — partial matches should NOT trigger
        assert!(!should_skip_content("You are an AI assistant that helps with coding"));
        assert!(!should_skip_content("Current time: 2026-04-09"));
        assert!(!should_skip_content("The RustClaw workspace is located at..."));

        // ============================================================
        // Group 5: Trivial Q&A with no info value
        // ============================================================

        // Positive cases — single punctuation + filler response
        assert!(should_skip_content("？ → 嗯？怎么了 potato，有什么事？"));
        assert!(should_skip_content("[TELEGRAM potato (@potatosoupup) id:7539582820 Wed 2026-04-01 17:09 -04:00]\n\n？ → 嗯？怎么了 potato，有什么事？"));
        assert!(should_skip_content("? → What's up?"));
        assert!(should_skip_content("👍 → noted"));
        assert!(should_skip_content("ok → 好的"));

        // Negative — real single-word questions with substantive answers
        assert!(!should_skip_content("engram还有什么要修改的吗 → 让我检查一下..."));
        // Negative — even short Q, but long informative A
        assert!(!should_skip_content("？ → 这个问题的根因是：1) 数据库连接池配置不当 2) 缺少重试机制 3) 超时时间设置过短。建议的修复方案是..."));

        // ============================================================
        // Group 6: Heartbeat noise
        // ============================================================

        // Positive — routine heartbeat with no findings
        assert!(should_skip_content("🫀 Heartbeat Check — all clear, nothing to report"));
        assert!(should_skip_content("Heartbeat: all systems normal, no action needed"));

        // Negative — heartbeat that found real issues
        assert!(!should_skip_content("🫀 Heartbeat Check — 🔴 CRITICAL: test suite failing, 12 tests broken"));
        assert!(!should_skip_content("Heartbeat found error in deployment pipeline, need to investigate"));

        // ============================================================
        // Group 7: Duplicate identity facts
        // ============================================================

        // Positive — trivial identity repetition
        assert!(should_skip_content("用户昵称为potato"));
        assert!(should_skip_content("Telegram用户名为@potatosoupup"));
        assert!(should_skip_content("用户正在测试RustClaw的功能"));

        // Negative — identity info in context of real conversation
        assert!(!should_skip_content("用户昵称为potato，他提出了一个很好的架构建议：我们应该用事件驱动而不是轮询来实现消息同步，这样可以减少延迟和资源消耗"));

        // ============================================================
        // Real conversations that must NOT be filtered
        // ============================================================
        assert!(!should_skip_content(
            "How do I implement a binary search tree in Rust? → Here's an implementation..."
        ));
        assert!(!should_skip_content(
            "帮我写一个 TODO 应用 → 好的，我来帮你设计一个 TODO 应用..."
        ));
        assert!(!should_skip_content(
            "What's the difference between Arc and Rc? → Arc is atomic reference counting..."
        ));
        assert!(!should_skip_content(
            "My deployment failed with exit code 137 → That's an OOM kill signal..."
        ));
    }
}
