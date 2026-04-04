# DESIGN: Session Resilience — Breakpoint Resume & Tool Output Persistence

## Problem

When RustClaw's LLM API calls fail mid-turn (network errors, rate limits, 5/5 retries exhausted), two things break:

1. **Content loss**: LLM was generating text/planning a tool call → connection drops → the partial thinking is lost. Next turn starts from scratch without knowing what was happening.
2. **Tool output loss**: Previous tool results (file reads, command outputs) are only in session memory. If session gets summarized or crashes, detailed tool outputs are gone.

**Real example** (2026-04-04 04:03-04:05 UTC): RustClaw hit `error sending request` 4 times. The document LLM was about to write was never saved because the `write_file` tool call never executed.

## Design

### Feature 1: Breakpoint Resume

**Goal**: When an LLM call fails after all retries, preserve context so the next turn can resume seamlessly.

#### Current Flow (broken)
```
Turn N: LLM call → 5 retries fail → Err(e) → return Err → session saves messages up to Turn N-1 → user sees error → next message starts fresh
```

#### New Flow
```
Turn N: LLM call → 5 retries fail → inject resume message into session → save session → notify user → next message picks up where we left off
```

#### Implementation

In `agent.rs` `run_agent_loop()`, after the LLM call fails with a non-413 error:

```rust
Err(e) => {
    // NEW: Instead of returning error immediately, inject a resume context
    tracing::error!("Turn {}: LLM call failed after retries: {}", turn, e);
    
    // Build resume context from the last assistant message + tool results
    let resume_ctx = build_resume_context(&session.messages, turn);
    
    // Inject as a system-style user message so next turn has context
    session.messages.push(Message::text("user", &format!(
        "[SYSTEM] Previous LLM call failed ({}). Session preserved. \
         When you resume, continue from where you left off:\n{}",
        e, resume_ctx
    )));
    
    // Save session immediately (don't lose the resume context)
    // Use if-let — if save also fails, log but don't propagate (we're already in error recovery)
    if let Err(save_err) = self.sessions.save(&session).await {
        tracing::error!("Failed to save resume context: {} (original error: {})", save_err, e);
    }
    
    // Notify user
    // Truncate error message for user-facing notification
    let error_summary = {
        let s = e.to_string();
        if s.len() > 100 { format!("{}...", &s[..s.floor_char_boundary(100)]) } else { s }
    };
    let _ = tx.send(AgentEvent::Response(format!(
        "⚠️ API connection lost ({}). Your work is saved — send any message to resume.",
        error_summary
    ))).await;
    sent_response = true;
    break;
}
```

`build_resume_context()` extracts from recent messages:
- Last assistant text (partial thinking)
- Last tool calls made and their results
- What phase the agent was in (if ritual is active)

```rust
fn build_resume_context(messages: &[Message], turn: usize) -> String {
    let mut ctx = format!("Turn {} of agent loop.\n", turn);
    
    // Find last assistant message
    for msg in messages.iter().rev() {
        if msg.role == "assistant" {
            if let Some(text) = &msg.text_content() {
                let preview = truncate_safe(text, 500);
                ctx.push_str(&format!("Last assistant response: {}\n", preview));
            }
            // Check for tool calls
            if let Some(tool_calls) = msg.tool_calls() {
                for tc in tool_calls {
                    ctx.push_str(&format!("Pending tool: {} ({})\n", tc.name, 
                        truncate_safe(&tc.input.to_string(), 100)));
                }
            }
            break;
        }
    }
    
    // Find last tool results
    for msg in messages.iter().rev() {
        if msg.has_tool_results() {
            ctx.push_str("Last tool results were already processed.\n");
            break;
        }
    }
    
    ctx
}
```

#### Key Design Choices
- **No retry loop escalation**: 5 retries is enough. If all fail, save state and wait for user.
- **Resume is passive**: We don't auto-retry on the next heartbeat. User sends a message → agent naturally picks up from the injected context.
- **Session is saved immediately**: Even if the process crashes right after, the resume context is in SQLite.

### Feature 2: Tool Output Persistence (Execution Log)

**Goal**: Every tool execution is logged to a durable file, so even if session memory is compacted/lost, the record of what was done persists.

#### Execution Log Format

File: `.rustclaw/execution-log.jsonl` (append-only, one JSON per line)

```jsonl
{"ts":"2026-04-04T04:03:14Z","session":"telegram:7539582820","turn":3,"tool":"write_file","input":{"path":"DESIGN.md"},"output_len":5234,"error":false,"duration_ms":45}
{"ts":"2026-04-04T04:03:15Z","session":"telegram:7539582820","turn":3,"tool":"exec","input":{"command":"cargo build"},"output_len":1200,"error":false,"duration_ms":8500}
```

#### Implementation

In `agent.rs`, after each tool execution in both `run_agent_loop()` and `process_with_subagent()`:

```rust
// After tool execution
let result = self.tools.execute(&tc.name, tc.input.clone()).await;

// NEW: Log to execution log
log_tool_execution(
    session_key,
    turn,
    &tc.name,
    &tc.input,
    &result,
    start.elapsed(),
);
```

```rust
fn log_tool_execution(
    session_key: &str,
    turn: usize,
    tool_name: &str,
    input: &Value,
    result: &Result<ToolResult>,
    duration: std::time::Duration,
) {
    // Compact input: only include key fields, not full content
    let compact_input = match tool_name {
        "write_file" | "edit_file" => {
            // Only log path, not content (could be huge)
            let mut m = serde_json::Map::new();
            if let Some(p) = input.get("path") { m.insert("path".into(), p.clone()); }
            Value::Object(m)
        }
        "exec" => {
            let mut m = serde_json::Map::new();
            if let Some(c) = input.get("command") { m.insert("command".into(), c.clone()); }
            Value::Object(m)
        }
        _ => {
            // For reads etc, log full input (usually just a path)
            input.clone()
        }
    };

    let entry = serde_json::json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "session": session_key,
        "turn": turn,
        "tool": tool_name,
        "input": compact_input,
        "output_len": match result {
            Ok(r) => r.output.len(),
            Err(_) => 0,
        },
        "error": match result {
            Ok(r) => r.is_error,
            Err(_) => true,
        },
        "duration_ms": duration.as_millis(),
    });

    // Append to log file (fire-and-forget, never fail the tool)
    let Some(home) = dirs::home_dir() else { return; };
    let log_path = home.join(".rustclaw/execution-log.jsonl");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true).append(true).open(&log_path)
    {
        use std::io::Write;
        let _ = writeln!(f, "{}", entry);
    }
}
```

#### Log Rotation

Execution log grows unbounded. Simple rotation:
- On startup, check file size
- If > 10MB, rename to `execution-log.{date}.jsonl`, start fresh
- Keep last 7 days of logs

```rust
fn rotate_execution_log() {
    let Some(home) = dirs::home_dir() else { return; };
    let log_path = home.join(".rustclaw/execution-log.jsonl");
    if let Ok(meta) = std::fs::metadata(&log_path) {
        if meta.len() > 10_000_000 {
            let now = chrono::Local::now();
            let archive = log_path.with_file_name(
                format!("execution-log.{}.jsonl", now.format("%Y%m%d-%H%M%S"))
            );
            let _ = std::fs::rename(&log_path, &archive);
            // Cleanup archives older than 7 days
            if let Ok(entries) = std::fs::read_dir(home.join(".rustclaw")) {
                let cutoff = now - chrono::Duration::days(7);
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with("execution-log.") && name.ends_with(".jsonl") && name != "execution-log.jsonl" {
                        // Parse date from filename, delete if old
                        if let Ok(modified) = entry.metadata().and_then(|m| m.modified()) {
                            if let Ok(age) = modified.elapsed() {
                                if age > std::time::Duration::from_secs(7 * 86400) {
                                    let _ = std::fs::remove_file(entry.path());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
```

### Integration Points

| Component | Change |
|---|---|
| `agent.rs` `run_agent_loop()` | Breakpoint resume on LLM error; tool execution logging |
| `agent.rs` `process_with_subagent()` | Same tool execution logging |
| `agent.rs` startup | Log rotation check |
| `session.rs` | No changes (session save already works) |
| `llm.rs` | No changes (retry logic stays the same) |

### Coverage: All Agent Loops

Both features apply to ALL three agent loops in RustClaw:

| Loop | File | Breakpoint Resume | Execution Log |
|---|---|---|---|
| Main agent | `agent.rs` `run_agent_loop()` | ✅ | ✅ |
| Sub-agent | `agent.rs` `process_with_subagent()` | ❌ Not needed | ✅ |
| Ritual skill | `ritual_adapter.rs` `run_skill()` | ⚠️ Partial — ritual has own retry via state machine | ✅ |

For ritual skills: the state machine already handles SkillFailed → retry. The execution log captures tool calls. No separate resume injection needed — the ritual state file IS the resume point.

For sub-agents: breakpoint resume is not needed because each `spawn_specialist` creates a fresh session. The sub-agent's failure is reported back to the main agent (via tool result or notification), and the main agent's own session persists the context. The execution log still captures all tool calls for audit.

### Execution Log: Success Confirmation

For write operations, the log entry includes `"error": false` which confirms the tool completed. Combined with the `"input": {"path": "..."}`, we know:
- What file was written
- Whether it succeeded
- When it happened

If we need the actual content, it's in the file itself (since the write succeeded). If the write failed, `"error": true` tells us to check.

### What This Does NOT Do

- **No automatic retry after resume**: User must send a message to continue. This is intentional — automatic retry could burn tokens on a broken API.
- **Resume context survives compaction**: The injected resume message is short (~200-500 chars) and recent (last message). Auto-compact keeps the most recent messages, so the resume context will survive even if compaction triggers on the next turn. If context is critically full, the resume message replaces summarized history — which is the correct tradeoff (recent intent > old detail).
- **No streaming partial saves**: If LLM is mid-stream generating text and crashes, that partial text is lost. Only completed tool calls and messages are saved. (Streaming partial save would require streaming API changes — future work.)
- **No cross-process recovery**: If the RustClaw process itself crashes (OOM, panic), the resume context in the last `session.save()` is the recovery point. The execution log provides audit trail.

### Token Cost

- Breakpoint resume: ~0 extra tokens (just a message injection, no LLM call)
- Execution log: ~0 extra tokens (file I/O only, no LLM)
- The resume context message adds ~200-500 tokens to the next turn's input — negligible.

### Files Changed

| File | Lines | Description |
|---|---|---|
| `src/agent.rs` | +60 | Resume injection, log_tool_execution calls, rotation |
| (new) `src/execution_log.rs` | +80 | log_tool_execution(), rotate_execution_log(), build_resume_context() |

~140 lines total.
