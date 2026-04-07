# Sub-Agent Structured Error Handling

## Problem

Sub-agent failures are opaque `Err(anyhow::Error)` strings. Callers can't distinguish "retry-able" from "escalate to user." Main agent gets confused, reports vague errors, or appears stuck.

Also: successful sub-agents only return the last assistant message. Parent doesn't know which files were modified without reading the whole transcript.

## Design

### SubAgentResult — always returned, never Err

`run_subagent` always returns `Ok(SubAgentResult)`. Errors are in `result.outcome`. This ensures callers always get partial progress (tokens, turns, transcript, files modified) even on failure.

```rust
pub struct SubAgentResult {
    pub agent_id: String,
    /// Last assistant text output.
    pub output: String,
    /// Total tokens used across all LLM calls.
    pub tokens: u64,
    /// Number of agentic loop turns completed.
    pub turns: u32,
    /// Path to JSONL transcript (debug/audit).
    pub transcript_path: PathBuf,
    /// Files written or edited by the sub-agent.
    pub files_modified: Vec<String>,
    /// Structured outcome — what happened.
    pub outcome: SubAgentOutcome,
}

pub enum SubAgentOutcome {
    /// Completed normally (LLM returned end_turn with no tool calls).
    Completed,
    /// Auth failure — all tokens/profiles exhausted. User must re-login.
    AuthFailed(String),
    /// Rate limited (429/529). Caller may retry after delay.
    RateLimited(String),
    /// Request too large for API. Caller should reduce context or split task.
    ContextTooLarge,
    /// Hit max iterations without LLM producing a final response.
    MaxIterations,
    /// Wall-clock timeout exceeded.
    Timeout { elapsed_secs: u64 },
    /// User or system cancelled.
    Cancelled,
    /// Pre-execution failure (workspace load, client creation) or other unclassified error.
    Error(String),
}

impl SubAgentOutcome {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Completed)
    }

    /// Whether the caller should retry (possibly after a delay).
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::RateLimited(_))
    }

    /// Whether the error should be escalated to the user.
    pub fn should_escalate(&self) -> bool {
        matches!(self, Self::AuthFailed(_) | Self::Timeout { .. } | Self::Error(_))
    }

    /// Human-readable summary for notifications.
    pub fn display(&self) -> String {
        match self {
            Self::Completed => "Completed".into(),
            Self::AuthFailed(msg) => format!("Auth failed: {}", msg),
            Self::RateLimited(msg) => format!("Rate limited: {}", msg),
            Self::ContextTooLarge => "Context too large for API".into(),
            Self::MaxIterations => "Hit max iterations without completing".into(),
            Self::Timeout { elapsed_secs } => format!("Timeout after {}s", elapsed_secs),
            Self::Cancelled => "Cancelled".into(),
            Self::Error(msg) => format!("Error: {}", msg),
        }
    }
}
```

### Internal Architecture: LoopResult + classify_error

`process_with_subagent` returns a private `LoopResult` — it does NOT do error classification. Classification happens only in `run_subagent`, keeping the blast radius minimal.

```rust
/// Internal result from the agentic loop. Not public.
struct LoopResult {
    output: String,
    turns: u32,
    files_modified: Vec<String>,
}

// process_with_subagent signature (minimal change from current):
async fn process_with_subagent(...) -> anyhow::Result<LoopResult>
//  Was: -> anyhow::Result<String>
//  Only adds turns + files_modified tracking to existing loop. No error classification.

// run_subagent wraps everything:
pub async fn run_subagent(...) -> SubAgentResult {
    let agent_id = generate_id();
    // Pre-execution: workspace, client, tools — any failure → Error outcome
    let subagent = match setup() {
        Ok(s) => s,
        Err(e) => return SubAgentResult { outcome: Error(e.to_string()), turns: 0, .. },
    };
    // Execution: run the loop — classify errors on failure
    match self.process_with_subagent(&subagent, ...).await {
        Ok(loop_result) => SubAgentResult { outcome: Completed, output: loop_result.output, .. },
        Err(e) => SubAgentResult { outcome: classify_error(&e), output: partial, .. },
    }
}

/// Classify an anyhow::Error into a SubAgentOutcome.
/// Single function, one place to update when error formats change.
fn classify_error(e: &anyhow::Error) -> SubAgentOutcome { ... }
```

Orchestrator and other direct callers of `process_with_subagent` continue working — they just get `LoopResult` instead of `String` (add `.output` to extract the string). Minimal breakage.

### Error Classification

`classify_error` matches error strings in one place:

| Error Source | Classification |
|-------------|---------------|
| `chat_stream` returns 401 after all profiles tried | `AuthFailed` |
| `chat_stream` returns 429/529 | `RateLimited` |
| `is_prompt_too_long(&e)` match | `ContextTooLarge` |
| `cancel_token.is_cancelled()` | `Cancelled` |
| Wall-clock > limit | `Timeout` |
| Loop exhausts `max_iterations` | `MaxIterations` |
| `e.is_timeout()` (HTTP timeout) | `Timeout` |
| Everything else | `Error(e.to_string())` |

### files_modified tracking

`process_with_subagent` tracks which files the sub-agent wrote/edited:

```rust
// After each tool call, track only successful writes:
let result = subagent.tools.execute(&tc.name, tc.input.clone()).await;
if !result.is_error && (tc.name == "write_file" || tc.name == "edit_file") {
    if let Some(path) = tc.input.get("path").and_then(|v| v.as_str()) {
        files_modified.push(path.to_string());
    }
}
```

Deduped before returning. Parent sees exactly which files were successfully changed.

### Caller Behavior

**SpawnTool (fire-and-forget):**
```rust
let result = runner.run_subagent(agent_type, &task, options).await;
match &result.outcome {
    Completed => notify("✅ Sub-agent completed"),
    AuthFailed(msg) => notify("🔒 Sub-agent auth failed — please re-login"),
    RateLimited { .. } => { sleep(retry_after); retry(); },
    MaxIterations => notify("⚠️ Sub-agent hit iteration limit, partial work done"),
    Timeout { .. } => notify("⏰ Sub-agent timed out"),
    _ => notify("❌ Sub-agent failed"),
}
// Always include: result.files_modified, result.tokens, result.turns
```

**SpawnTool (wait mode):**
```rust
let result = runner.run_subagent(agent_type, &task, options).await;
Ok(ToolResult {
    output: format!(
        "## Sub-agent '{}' — {}\n\nOutput: {}\n\nFiles modified: {:?}\nTokens: {}, Turns: {}",
        result.agent_id, result.outcome.display(),
        result.output, result.files_modified, result.tokens, result.turns
    ),
    is_error: !result.outcome.is_success(),
})
```

**Ritual runner:**
```rust
let result = runner.run_subagent(agent_type, context, options).await;
if result.outcome.is_success() {
    Ok((RitualEvent::SkillCompleted { phase, artifacts: result.files_modified }, result.tokens))
} else {
    Ok((RitualEvent::SkillFailed { phase, error: result.outcome.display() }, result.tokens))
}
```

### What Changes

| File | Change |
|------|--------|
| `agent.rs` | Add `SubAgentOutcome` enum, `classify_error()` fn, private `LoopResult` struct. |
| `agent.rs` | `SubAgentResult` gains `outcome: SubAgentOutcome` and `files_modified: Vec<String>`. |
| `agent.rs` | `process_with_subagent` returns `Result<LoopResult>` (was `Result<String>`). Tracks turns + files_modified inside loop. No error classification. |
| `agent.rs` | `run_subagent` returns `SubAgentResult` (not `Result<SubAgentResult>`). Wraps pre-execution and execution errors via `classify_error`. |
| `tools.rs` | SpawnTool uses `result.outcome` for error handling and display. |
| `ritual_runner.rs` | Ritual runner uses `result.outcome` for SkillCompleted/SkillFailed. |
| `orchestrator.rs` | Add `.output` to `process_with_subagent` call (LoopResult → String). |
| `llm.rs` | No changes — the 401 bail-fast fix is already in place. |

### What Doesn't Change

- LLM client internals (chat, chat_stream, retry logic)
- The agentic loop structure (turns, tool execution, compaction)
- TranscriptWriter, AgentType, ToolRegistry
- Ritual advance model

### Risks

| Risk | Mitigation |
|------|-----------|
| `process_with_subagent` returns `LoopResult` not `String` | Minimal: orchestrator adds `.output`. All run_subagent callers unaffected. |
| Error classification might be wrong (e.g., timeout misclassified as Error) | Classification uses explicit checks: `e.is_timeout()`, `contains("401")`, `is_prompt_too_long()`. Fallback is `Error(msg)` — safe default. |
| `files_modified` tracking misses some writes (e.g., shell `echo > file`) | Only tracks write_file/edit_file tool calls. Exec-based writes are not tracked. Acceptable — if you use exec to write files, you know what you did. |
