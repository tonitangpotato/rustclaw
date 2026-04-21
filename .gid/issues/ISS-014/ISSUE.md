# ISS-014: Session Continuity — Recent Memories Not Reaching LLM After Restart

**Created**: 2026-04-20
**Priority**: High
**Status**: Analysis Complete → Ready for Fix

---

## Problem

After every restart, the agent behaves as if it has no context about what was happening right before the restart. User has to re-explain what they were doing. This breaks the illusion of continuity and wastes time.

## What Already Exists (Surprising Amount)

Investigation of the codebase reveals that **most of the machinery is already built**:

| Component | Status | Location |
|-----------|--------|----------|
| `recall_recent(limit)` in engramai | ✅ Exists | `engramai/src/memory.rs:2015` |
| `MemoryManager::recall_recent()` wrapper | ✅ Exists | `src/memory.rs:614` |
| `MemoryManager::format_recent_for_prompt()` | ✅ Exists | `src/memory.rs:636` |
| Config: `recent_memory_limit: 50` | ✅ Set | `rustclaw.yaml:42` |
| Fresh session detection + injection | ✅ Exists | `src/agent.rs:905-929` |
| Session starts fresh (no DB restore) | ✅ By design | `src/session.rs:329` |

The code already:
1. Detects fresh sessions (`session.messages.is_empty()`)
2. Calls `recall_recent(50)`
3. Formats them as `## 🧠 Recent Memories (session startup)`
4. Appends to system prompt

**So why isn't it working?**

## Root Cause Analysis

### Hypothesis 1: `session.messages.is_empty()` is never true after restart

The session is keyed by `session_key` (likely `telegram:{chat_id}`). After restart:
- In-memory `HashMap<String, Session>` is empty ✅ (process died)
- `get_or_create()` creates a new empty session ✅ (line 331: "Create new — do NOT restore from DB")
- So `session.messages.is_empty()` should be `true` on first message after restart ✅

**Verdict: This should work. Need to verify with logs.**

### Hypothesis 2: recall_recent returns empty or low-quality results

`recall_recent` returns `MemoryRecord` sorted by `created_at DESC`. The records include:
- Auto-stored memories (from BeforeOutbound hook)
- Explicit `engram_store` calls
- Both have `content`, `memory_type`, `created_at`, `source`

But `format_recent_for_prompt` outputs:
```
- [Episodic] Some memory content here
- [Factual] Another memory
```

**No timestamps.** The LLM sees 50 memories but has no idea which ones are from 5 minutes ago vs 3 days ago. They all look the same. The most recent (most relevant for continuity) gets lost in a wall of 50 undifferentiated memories.

**Verdict: Likely contributing. Timestamps are critical.**

### Hypothesis 3: 50 memories is too many, drowning the signal

50 recent memories ≈ 5,000-10,000 tokens of context. That's a lot of noise. The truly useful continuity info (what was being worked on RIGHT NOW) is maybe 3-5 memories, buried in 45 others.

**Verdict: Likely contributing. Need smarter selection.**

### Hypothesis 4: Auto-stored memories are too granular/noisy

The auto-store hook fires on every LLM response. If the agent is doing a multi-step task (read file → edit → read again → compile), each step generates a memory. The "I was fixing the knowledge compiler ID collision bug" signal is spread across dozens of low-level operational memories like "Read file src/compiler/api.rs" and "Edited line 42".

**Verdict: Likely the core issue. Auto-stored memories are operational logs, not working-state summaries.**

### Hypothesis 5: Not actually reaching system prompt

The injection happens at `src/agent.rs:1012-1015`:
```rust
if !recent_memory_context.is_empty() {
    system_prompt.push_str("\n");
    system_prompt.push_str(&recent_memory_context);
}
```

This is AFTER the memory_context (hook-based recall) and interoceptive state. In a long system prompt, this section might be at the very end, potentially getting truncated or deprioritized by the LLM.

**Verdict: Position might matter. But should still work.**

## Confirmed Root Cause

The combination of H2 + H3 + H4:

1. **No timestamps** → LLM can't distinguish recent from old
2. **Too many memories (50)** → Signal drowned in noise
3. **Memories are operational, not contextual** → "Read file X" is not "I was debugging the knowledge compiler"

## Proposed Fix

### Fix 1: Add timestamps to `format_recent_for_prompt` (Essential)

Change the format from:
```
- [Episodic] Some memory content
```
to:
```
- [15:10] [Episodic] Some memory content
- [15:08] [Factual] Another memory
- [14:55] [Episodic] Older memory
- [yesterday 22:30] [Episodic] Even older
```

Relative timestamps ("5min ago", "2h ago", "yesterday") are even better for LLM comprehension but harder to implement. Simple `HH:MM` or `YYYY-MM-DD HH:MM` is fine.

**Implementation**: `format_recent_for_prompt` in `src/memory.rs:636`. The `MemoryRecord` already has `created_at: DateTime<Utc>`. We just need to pass it through via `RecalledMemory` (currently drops the timestamp).

### Fix 2: Reduce limit + smart filtering (Essential)

Instead of dumping 50 raw memories:
- **Reduce to 15-20 most recent** (configurable, keep `recent_memory_limit`)
- **Deduplicate**: consecutive memories about the same topic → keep the most informative one
- **Filter out pure operational noise**: memories that are just "Read file X" or "Executed command Y" add noise, not context

The filtering can be simple heuristics:
- Skip memories shorter than 20 chars
- Skip memories that start with common operational prefixes ("Read file", "Listed directory", etc.)
- Group consecutive memories from the same source

### Fix 3: Add `created_at` to `RecalledMemory` struct (Required for Fix 1)

Currently `RecalledMemory` has:
```rust
pub struct RecalledMemory {
    pub content: String,
    pub memory_type: String,
    pub confidence: f64,
    pub source: Option<String>,
    pub confidence_label: Option<String>,
}
```

Needs:
```rust
    pub created_at: Option<DateTime<Utc>>,
```

This is needed so `format_recent_for_prompt` can show timestamps. The data exists in `MemoryRecord.created_at` — just not passed through the mapping in `recall_recent()`.

### Fix 4: System prompt instruction (Quick Win)

Add a line to the system prompt behavior section:
```
When you see "Recent Memories (session startup)" in your context, 
proactively summarize what you were last working on and ask the user 
if they want to continue. Don't wait for them to ask.
```

This costs nothing and can work immediately even before the code fixes.

## Non-Goals

- **Not building a "working state" file** — that's manual maintenance, not a root fix
- **Not changing how auto-store works** — the memories being stored are fine, the problem is how they're presented at startup
- **Not adding a pre-restart save hook** — must handle crashes too, not just clean restarts

## Implementation Order

1. Fix 3 (add `created_at` to RecalledMemory) — prerequisite
2. Fix 1 (timestamps in format) — highest impact
3. Fix 2 (reduce limit + filter) — signal-to-noise
4. Fix 4 (system prompt instruction) — immediate, can do right now

## Files to Change

- `src/memory.rs` — RecalledMemory struct, recall_recent mapping, format_recent_for_prompt
- `src/workspace.rs` or system prompt template — behavior instruction
- `rustclaw.yaml` — maybe reduce recent_memory_limit from 50 to 20
- No engramai changes needed — MemoryRecord already has all the data

## Verification

After fix:
1. Restart RustClaw
2. Send "hi" or "继续"
3. Agent should respond with something like: "我回来了。刚才在处理 knowledge compiler 的 ID collision bug，已经修好并编译了，需要重启生效。要我继续跑 knowledge_compile 吗？"
