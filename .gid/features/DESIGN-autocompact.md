# DESIGN-autocompact.md — Auto-Compact for Continuous Agentic Coding

## Problem

RustClaw's current context management triggers summarization based on **message count** (`max_session_messages`), not token count. This causes:

1. **Premature compaction** — 20 short messages with small tool results get compacted even though they fit easily in context
2. **Missed compaction** — 5 messages with 50KB tool results each overflow context before message-count threshold triggers
3. **No recovery from 413** — if context overflows mid-loop, the request fails and the agentic loop breaks
4. **No max_output_tokens escalation** — truncated output gets a generic retry, no 8K→64K ramp

The result: RustClaw can run ~20-30 turns before context issues halt it. Claude Code runs 100+ turns continuously for hours.

## Current State (What RustClaw Has)

| Feature | Status | Location |
|---------|--------|----------|
| Message-count summarization | ✅ Working | `session.rs:487` |
| Microcompact (old tool results) | ✅ Working | `session.rs` |
| Persist-to-disk (large results) | ✅ Working | `agent.rs` |
| max_tokens retry | ✅ Basic | `agent.rs:600` |
| Prompt caching | ✅ Working | `llm.rs` |
| Token counting | ❌ Missing | — |
| Token-based compaction trigger | ❌ Missing | — |
| 413 recovery | ❌ Missing | — |
| Output token escalation | ❌ Missing | — |

## Design (What to Add)

### 1. Token Counting

Add approximate token counting to `Session`:

```rust
impl Session {
    /// Estimate total tokens in current messages.
    /// Uses chars/4 heuristic (close enough for threshold decisions).
    fn estimate_tokens(&self) -> usize {
        let mut total = 0;
        for msg in &self.messages {
            total += msg.content_chars() / 4;
        }
        total
    }
}
```

**Why chars/4?** Claude Code uses the same heuristic for threshold checks. Exact tokenization would require tiktoken/sentencepiece, adding ~10ms per check. chars/4 is <1ms and accurate enough for "should we compact?" decisions.

### 2. Token-Based Compaction Trigger

Replace message-count trigger with token-based:

```rust
// In agent.rs agentic loop, before each LLM call:
let estimated_tokens = session.estimate_tokens();
let model_limit = self.get_model_context_limit(); // 200K for Opus, 200K for Sonnet

// Compact at 80% of limit (leave headroom for response + tools)
let compact_threshold = (model_limit as f64 * 0.80) as usize;

if estimated_tokens > compact_threshold {
    match self.auto_compact(&mut session, &system_prompt).await {
        Ok(summary) => {
            tracing::info!("Auto-compacted: {} → {} tokens", estimated_tokens, session.estimate_tokens());
        }
        Err(e) => {
            tracing::warn!("Auto-compact failed: {}", e);
            // Fallback: aggressive trim
            session.trim_messages(10);
        }
    }
}
```

### 3. Improved Compaction Prompt

Current prompt is too simple ("summarize in a paragraph"). CC's approach preserves structure:

```rust
const COMPACT_SYSTEM: &str = r#"You are a conversation summarizer. Create a structured summary that preserves:
1. The original task/goal
2. Key decisions made
3. Current progress and state
4. File paths, function names, and code identifiers mentioned
5. Any errors encountered and their resolutions
6. What was being worked on when compaction triggered

Format as a structured summary with sections, not a paragraph. Be thorough — this summary replaces the full conversation history."#;
```

### 4. Compaction Implementation

```rust
impl AgentRunner {
    async fn auto_compact(
        &self,
        session: &mut Session,
        system_prompt: &str,
    ) -> anyhow::Result<()> {
        // Use summary LLM (cheaper model) if available, otherwise main LLM
        let llm = self.summary_llm.as_deref()
            .unwrap_or_else(|| self.llm_client.blocking_read());
        
        // Keep last N messages as "tail" (recent context)
        let keep_recent = 6; // 3 assistant + 3 user/tool_result pairs
        let (to_summarize, count) = session.prepare_for_summarization_by_tokens(
            keep_recent,
        )?;
        
        let conversation_text = format_messages_for_summary(&to_summarize);
        
        let response = llm.chat(
            COMPACT_SYSTEM,
            &[Message::text("user", format!(
                "Summarize this conversation:\n\n{}\n\nPreserve all technical details, file paths, and current state.",
                conversation_text
            ))],
            &[],
        ).await?;
        
        let summary = response.text.unwrap_or_default();
        session.apply_summary(&summary, count);
        
        Ok(())
    }
}
```

### 5. 413 Recovery (Reactive Compact)

When the API returns 413 (prompt too long), compact and retry:

```rust
// In the agentic loop, wrap the LLM call:
let response = match llm_guard.chat(&system_prompt, &session.messages, &tool_defs).await {
    Ok(r) => r,
    Err(e) if is_prompt_too_long(&e) => {
        tracing::warn!("413 prompt too long — triggering reactive compact");
        drop(llm_guard);
        self.auto_compact(&mut session, &system_prompt).await?;
        let llm_guard = self.llm_client.read().await;
        llm_guard.chat(&system_prompt, &session.messages, &tool_defs).await?
    }
    Err(e) => return Err(e),
};
```

### 6. Output Token Escalation

Replace the current basic retry with CC's escalation pattern:

```rust
if response.stop_reason == "max_tokens" {
    if !attempted_escalation {
        // First: retry with higher max_tokens (8K → 64K)
        tracing::info!("max_tokens hit — escalating to 64K output");
        attempted_escalation = true;
        // Retry same request with max_output_tokens=64000
        continue;
    }
    
    if max_tokens_recovery_count < 3 {
        // Second: inject resume prompt
        max_tokens_recovery_count += 1;
        session.messages.push(Message::text("user",
            "Output token limit hit. Resume directly — no apology, no recap. \
             Pick up mid-thought. Break remaining work into smaller pieces."
        ));
        continue;
    }
    
    // Exhausted — give up
    tracing::error!("max_tokens recovery exhausted after 3 attempts");
}
```

## Implementation Plan

### Phase 1: Token Counting + Threshold (Core)
- Add `estimate_tokens()` to Session
- Add `get_model_context_limit()` to AgentRunner
- Replace message-count trigger with token-based in agentic loop
- Keep message-count as fallback (belt and suspenders)

### Phase 2: Better Compaction
- New structured compaction prompt
- `prepare_for_summarization_by_tokens()` — split by token budget, not count
- Test with real long sessions

### Phase 3: Recovery Mechanisms
- 413 reactive compact
- Output token escalation (8K → 64K → resume prompt)
- Track compaction count for telemetry

### Phase 4: Advanced (Future)
- Streaming tool execution (start tool while response still streaming)
- Memory prefetch (fetch engram memories while model streams)
- Context collapse (staged, per-section folding instead of full summary)

## Files to Modify

| File | Changes |
|------|---------|
| `src/session.rs` | `estimate_tokens()`, `prepare_for_summarization_by_tokens()` |
| `src/agent.rs` | Token-based compact trigger, 413 recovery, output escalation |
| `src/llm.rs` | `is_prompt_too_long()` error detection, model context limits |
| `rustclaw.yaml` | New config: `compact_threshold_pct` (default 0.80) |

## Config

```yaml
context:
  # Existing
  max_session_messages: 200
  microcompact_after: 4
  persist_threshold_bytes: 30000
  
  # New
  compact_threshold_pct: 0.80    # Compact at 80% of model's context limit
  compact_keep_recent: 6         # Messages to preserve in tail
  max_output_tokens_escalation: true
  reactive_compact: true         # Auto-compact on 413
```

## Key Differences from Claude Code

| Feature | Claude Code | RustClaw (proposed) |
|---------|-------------|---------------------|
| Token counting | Server-side usage + client estimate | Client-side chars/4 |
| Compact model | Same model (expensive) | Separate cheaper model ✅ |
| Context collapse | Feature-gated staged folding | Not planned (Phase 4) |
| Microcompact | By tool_use_id (cached) | By message age ✅ |
| Streaming tools | StreamingToolExecutor | Not planned (Phase 4) |
| Memory prefetch | Parallel during stream | Not planned (Phase 4) |

RustClaw's advantage: **separate summary LLM** — CC compacts with the same expensive model, RustClaw already uses a cheaper model for summarization. This saves significant tokens.
