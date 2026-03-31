# RustClaw Context Efficiency — Design Document

## Problem

RustClaw's context grows unbounded during conversations, reaching 105K+ tokens. Main causes:
1. No context compaction — old messages trimmed but not summarized
2. No tool result management — 44K char web fetches stay in context forever
3. No intermediate text delivery — agent can't acknowledge before tool execution
4. No message queue — user messages during tool loops are invisible to the agent

## Architecture (inspired by Claude Code source analysis)

### Phase 1: Microcompact (zero-cost context reduction)

Clear old tool result content without LLM calls. Replace with `[Tool result cleared — {n} chars]`.

**Logic:**
- After each LLM response, before next API call, scan message history
- For tool_result messages older than N turns (default: 3), replace content with cleared marker
- Preserve tool name + first 200 chars as preview
- Track tokens saved for logging

**Where:** New function `microcompact_messages()` in `src/session.rs`, called in agent loop before LLM request.

**Thresholds (from CC):**
- Per-tool result: clear if > 2K chars and older than 3 turns
- Per-message aggregate: if total tool results in a single turn > 50K chars, clear largest first

### Phase 2: Auto-compact (LLM-based summarization)

Enable the existing `summarize_old_messages()` by configuring a summary model.

**Config change:** Add `summary_model: claude-haiku-4-5` to rustclaw.yaml

**Trigger:** When message count exceeds `max_session_messages` (40), summarize old messages into a compact summary using Haiku.

**Already implemented** in `src/session.rs` and `src/agent.rs` — just needs config.

### Phase 3: Tool result persist-to-disk

For tool results exceeding a size threshold, persist full content to disk and replace in-context with preview + file path.

**Logic:**
- When a tool returns > 30K chars, write full result to `~/.rustclaw/tool-results/{session}/{tool_call_id}.txt`
- Replace in-context content with: first 2K chars + `\n\n[Full output: {path} ({n} chars) — use read_file to access]`
- Agent can read_file to get the full content if needed

**Where:** New module `src/tool_result_storage.rs`. Called in agent loop after tool execution, before adding result to messages.

### Phase 4: Event stream (intermediate text delivery)

Replace `process_message_with_options() -> String` with event-based output.

**Event types:**
```rust
pub enum AgentEvent {
    /// Intermediate text — send to user immediately
    Text(String),
    /// Tool execution starting
    ToolStart { name: String },
    /// Tool execution complete (for verbose mode)
    ToolResult { name: String, preview: String },
    /// Final response text
    Response(String),
    /// Error
    Error(String),
}
```

**New method:** `process_message_events() -> mpsc::Receiver<AgentEvent>`

**Agent loop change:** When LLM returns text + tool_calls, emit `AgentEvent::Text` before executing tools.

**Telegram change:** Consume events from channel. Send Text events immediately. Send Response as final reply.

### Phase 5: Message queue (BTW / steer)

Allow new messages to reach the agent during tool loops.

**Queue structure:**
```rust
pub struct MessageQueue {
    pending: Vec<QueuedMessage>,
}

pub struct QueuedMessage {
    text: String,
    priority: Priority, // Now, Next, Later
    timestamp: Instant,
}
```

**Injection point:** After tool results are collected each turn, check queue. If messages pending, inject as user message before next LLM call.

**BTW (side question):** If message starts with `/btw`, fork a lightweight session — share context snapshot, no tools, single turn, respond immediately without interrupting main loop.

**Where:** New module `src/message_queue.rs`. Queue lives on `AgentRunner` (per-session). Telegram pushes to queue when session is busy.

## Non-goals
- Coordinator mode (single-user agent, not needed)
- Prompt cache sharing (Anthropic API-level optimization, defer)
- Budget tracking (Max plan, not billing-relevant)

## Dependencies
- Phase 1: standalone
- Phase 2: standalone (config only)
- Phase 3: standalone
- Phase 4: depends on Phase 1-3 being stable
- Phase 5: depends on Phase 4

## Success metrics
- Main session context stays under 30K tokens (currently 105K)
- Sub-agent context stays under 50K tokens
- User sees acknowledge within 2s of sending message
- Messages during tool loops are not lost
